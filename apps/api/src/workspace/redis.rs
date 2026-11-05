//! Atomic Redis operations kept next to the Rust transport.

pub const PUBLISH: &str = r#"-- Small, expiring replay log + bounded current-state snapshots. Both keys use
-- the same account hash tag, so this remains atomic on Redis Cluster.
local frame=cjson.decode(ARGV[1])
local id=redis.call('XADD',KEYS[1],'MAXLEN',256,'*','payload',ARGV[1])
frame.live_cursor=id
local event=frame.event
local function truncate_utf8(value,limit)
  if string.len(value)<=limit then return value end
  while limit>0 do
    local next=string.byte(value,limit+1)
    if next<128 or next>=192 then break end
    limit=limit-1
  end
  return string.sub(value,1,limit)
end
local field=nil
if event.type=='ai_start' or event.type=='ai_delta' or event.type=='ai_done' then
  field='ai:'..event.stream_id
  local previous=redis.call('HGET',KEYS[2],field)
  local snapshot=previous and cjson.decode(previous) or frame
  local old=snapshot.event
  local content=old.content or ''
  local received=old.received_bytes or 0
  local missing=false
  local delta=type(event.delta)=='string' and event.delta or (type(event.delta)=='table' and event.delta.content or nil)
  if event.type=='ai_delta' and type(delta)=='string' then
    missing=type(event.offset)=='number' and event.offset~=received
    received=(event.offset or received)+string.len(delta)
    if not old.truncated and not missing then content=content..delta end
  end
  if event.type=='ai_done' and type(event.total_bytes)=='number' and event.total_bytes~=received then missing=true end
  frame.event={type='ai_snapshot',stream_id=event.stream_id,conversation_id=event.conversation_id or old.conversation_id,content=truncate_utf8(content,16384),received_bytes=received,prompt=event.prompt or old.prompt or '',done=event.type=='ai_done',complete=event.type=='ai_done' and event.complete==true,truncated=old.truncated or missing or string.len(content)>16384}
elseif event.type=='transcript' then
  local channel=event.result.channel_index and event.result.channel_index[1] or 0
  field='speech:'..tostring(frame.meeting_id)..':'..tostring(frame.device_id)..':'..channel
elseif event.type=='meeting_active' or event.type=='meeting_ended' then
  field='meeting:'..tostring(frame.meeting_id)..':'..tostring(frame.device_id)
end
if field then
  redis.call('HSET',KEYS[2],field,cjson.encode(frame))
  if redis.call('HLEN',KEYS[2])>16 then
    local oldest=nil
    local oldest_time=nil
    local values=redis.call('HGETALL',KEYS[2])
    for n=1,#values,2 do
      if values[n]~=field then
        local timestamp=cjson.decode(values[n+1]).emitted_at or 0
        if not oldest_time or timestamp<oldest_time then oldest=values[n]; oldest_time=timestamp end
      end
    end
    if oldest then redis.call('HDEL',KEYS[2],oldest) end
  end
end
redis.call('EXPIRE',KEYS[1],180)
redis.call('EXPIRE',KEYS[2],180)
-- Existing desktop listeners also consume these events, so preserve the wire shape.
local published=cjson.decode(ARGV[1])
published.live_cursor=id
redis.call('PUBLISH',ARGV[2],cjson.encode(published))
return id
"#;

pub const REPLAY: &str = r#"local after=ARGV[1]
local time=redis.call('TIME')
local now=tonumber(time[1])*1000+math.floor(tonumber(time[2])/1000)
local present=after~='0-0' and #redis.call('XRANGE',KEYS[1],after,after,'COUNT',1)>0
if present then
  local rows=redis.call('XRANGE',KEYS[1],'('..after,'+','COUNT',256)
  local frames={}
  local bytes=0
  for _,row in ipairs(rows) do
    bytes=bytes+string.len(row[2][2])
    if bytes>262144 then present=false; break end
    local frame=cjson.decode(row[2][2]); frame.live_cursor=row[1]; table.insert(frames,frame)
  end
  if present then return cjson.encode({type='replay',frames=frames,server_time=now}) end
end
local last=redis.call('XREVRANGE',KEYS[1],'+','-','COUNT',1)
local frames={}
local candidates={}
for _,raw in ipairs(redis.call('HVALS',KEYS[2])) do table.insert(candidates,cjson.decode(raw)) end
table.sort(candidates,function(a,b) return a.emitted_at>b.emitted_at end)
local bytes=0
for _,frame in ipairs(candidates) do
  local size=string.len(cjson.encode(frame))
  if bytes+size<=262144 then table.insert(frames,frame); bytes=bytes+size end
end
return cjson.encode({type='snapshot',live_cursor=#last>0 and last[1][1] or '0-0',frames=frames,server_time=now})
"#;
