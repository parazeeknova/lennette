import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";

import { API_URL, AccountError, accountRequest } from "@/lib/account-api";
import {
  applyLive,
  emptyLive,
  newerCursor,
  object,
  validCursor,
} from "@/lib/workspace";

export const useWorkspaceLive = (account: string, expired: () => void) => {
  const cache = useQueryClient();
  const [live, setLive] = useState(emptyLive);
  const [status, setStatus] = useState("Connecting");
  useEffect(() => {
    let cursor = "0-0";
    let retries = 0;
    let socket: WebSocket | undefined;
    let stopped = false;
    let clockOffset = 0;
    let refresh: ReturnType<typeof setTimeout> | undefined;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let animation: number | undefined;
    let pending: unknown[] = [];
    const invalidate = () => {
      if (refresh) {
        return;
      }
      refresh = setTimeout(() => {
        refresh = undefined;
        void cache.invalidateQueries({ queryKey: ["workspace", account] });
      }, 1000);
    };
    const normalize = (input: unknown) => {
      const frame = object(input);
      return {
        ...frame,
        emitted_at: Number(frame["emitted_at"]) + clockOffset,
      };
    };
    const reset = () => {
      cursor = "0-0";
      pending = [];
      if (animation !== undefined) {
        cancelAnimationFrame(animation);
        animation = undefined;
      }
      setLive(emptyLive());
      invalidate();
    };
    const consume = (raw: unknown) => {
      const frame = object(raw);
      const next = frame["live_cursor"];
      if (!validCursor(next) || !newerCursor(next, cursor)) {
        return;
      }
      if (pending.length >= 256) {
        reset();
        socket?.close();
        return;
      }
      cursor = next;
      pending.push(normalize(frame));
      if (animation !== undefined) {
        return;
      }
      animation = requestAnimationFrame(() => {
        const frames = pending;
        pending = [];
        animation = undefined;
        setLive((state) => {
          let nextState = state;
          for (const item of frames) {
            nextState = applyLive(nextState, item);
          }
          return nextState;
        });
      });
    };
    const checkSession = async () => {
      try {
        const user = await accountRequest<{ id: string }>("/me");
        if (user.id !== account && !stopped) {
          stopped = true;
          socket?.close();
          expired();
        }
      } catch (error) {
        if (error instanceof AccountError && error.status === 401 && !stopped) {
          stopped = true;
          socket?.close();
          expired();
        }
      }
    };
    const connect = () => {
      if (stopped) {
        return;
      }
      const url = new URL(`${API_URL}/v1/workspace/ws`);
      url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
      url.searchParams.set("live_after", cursor);
      url.searchParams.set("account", account);
      socket = new WebSocket(url);
      const current = socket;
      let connectedAt = Date.now();
      current.addEventListener("message", (message) => {
        if (
          stopped ||
          current !== socket ||
          typeof message.data !== "string" ||
          message.data.length > 1_048_576
        ) {
          return;
        }
        let frame: Record<string, unknown>;
        try {
          frame = object(JSON.parse(message.data));
        } catch {
          current.close();
          return;
        }
        if (typeof frame["server_time"] === "number") {
          clockOffset = Date.now() - frame["server_time"];
        }
        switch (frame["type"]) {
          case "ready": {
            connectedAt = Date.now();
            setStatus("Live");
            invalidate();
            break;
          }
          case "changed": {
            invalidate();
            break;
          }
          case "snapshot": {
            const frames = Array.isArray(frame["frames"])
              ? frame["frames"].slice(0, 16)
              : [];
            let next = emptyLive();
            for (const item of frames) {
              next = applyLive(next, normalize(item));
            }
            setLive(next);
            cursor = validCursor(frame["live_cursor"])
              ? frame["live_cursor"]
              : "0-0";
            break;
          }
          case "replay": {
            if (Array.isArray(frame["frames"])) {
              for (const item of frame["frames"].slice(0, 256)) {
                consume(item);
              }
            }
            break;
          }
          case "live": {
            consume(frame);
            break;
          }
          case "resync": {
            reset();
            current.close();
            break;
          }
          case "session_revoked": {
            stopped = true;
            current.close();
            expired();
            break;
          }
          default: {
            break;
          }
        }
      });
      current.addEventListener("error", () => {
        if (!stopped && current === socket) {
          setStatus("Reconnecting");
        }
      });
      current.addEventListener("close", () => {
        if (stopped || current !== socket) {
          return;
        }
        setStatus(navigator.onLine ? "Reconnecting" : "Offline");
        if (Date.now() - connectedAt > 30_000) {
          retries = 0;
        }
        const delay =
          Math.min(30_000, 500 * 2 ** Math.min(retries, 6)) +
          Math.random() * 500;
        retries += 1;
        retry = setTimeout(connect, delay);
        void checkSession();
      });
    };
    const resume = () => {
      if (document.hidden || stopped) {
        return;
      }
      void checkSession();
      invalidate();
      if (!socket || socket.readyState === WebSocket.CLOSED) {
        clearTimeout(retry);
        connect();
      }
    };
    let lastActivity = Date.now();
    const activity = () => {
      if (Date.now() - lastActivity < 60_000) {
        return;
      }
      lastActivity = Date.now();
      void accountRequest("/browser/activity", "POST").catch(resume);
    };
    connect();
    window.addEventListener("online", resume);
    document.addEventListener("visibilitychange", resume);
    window.addEventListener("pointerdown", activity);
    window.addEventListener("keydown", activity);
    return () => {
      stopped = true;
      clearTimeout(retry);
      clearTimeout(refresh);
      if (animation !== undefined) {
        cancelAnimationFrame(animation);
      }
      socket?.close();
      window.removeEventListener("online", resume);
      document.removeEventListener("visibilitychange", resume);
      window.removeEventListener("pointerdown", activity);
      window.removeEventListener("keydown", activity);
    };
  }, [account, cache, expired]);
  return { live, status };
};
