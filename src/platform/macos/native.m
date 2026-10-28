// Native macOS bridge. AppKit and Carbon entry points run on GPUI's main thread.
// ScreenCaptureKit owns a separate serial queue and a bounded PCM ring buffer.
#import <AppKit/AppKit.h>
#import <Carbon/Carbon.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreAudio/CoreAudioTypes.h>
#include <stdbool.h>
#include <stdint.h>

static NSWindow *window_for_view(void *view) {
    if (![NSThread isMainThread] || !view) return nil;
    return [(__bridge NSView *)view window];
}

bool lenny_macos_prepare_window(void *view) {
    NSWindow *window = window_for_view(view);
    if (!window) return false;
    // Best effort for legacy capture clients. ScreenCaptureKit can ignore this;
    // this must never be presented as universal full-display capture exclusion.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    window.sharingType = NSWindowSharingNone;
#pragma clang diagnostic pop
    // Lennette draws its own controls. Keep native resize/move behavior while
    // removing the traffic lights and the visible title from both windows.
    window.titleVisibility = NSWindowTitleHidden;
    window.titlebarAppearsTransparent = YES;
    window.styleMask |= NSWindowStyleMaskFullSizeContentView;
    // Apply after style changes: AppKit can recreate these buttons in setStyleMask.
    for (NSNumber *button in @[@(NSWindowCloseButton), @(NSWindowMiniaturizeButton), @(NSWindowZoomButton)]) {
        [window standardWindowButton:(NSWindowButton)button.unsignedIntegerValue].hidden = YES;
    }
    window.collectionBehavior |= NSWindowCollectionBehaviorCanJoinAllSpaces |
                                 NSWindowCollectionBehaviorFullScreenAuxiliary;
    return true;
}

bool lenny_macos_set_above(void *view, bool above) {
    NSWindow *window = window_for_view(view);
    if (!window) return false;
    window.level = above ? NSFloatingWindowLevel : NSNormalWindowLevel;
    return true;
}

bool lenny_macos_set_icon_hidden(bool hidden) {
    if (![NSThread isMainThread]) return false;
    return [NSApp setActivationPolicy:hidden ? NSApplicationActivationPolicyAccessory
                                            : NSApplicationActivationPolicyRegular];
}

// Carbon hotkeys do not monitor arbitrary keystrokes and require neither
// Accessibility nor Input Monitoring access. macOS reports conflicts at registration.
typedef struct {
    EventHandlerRef handler;
    EventHotKeyRef refs[64];
    uint32_t pending[64];
    size_t read, count;
} LennyHotkeys;

static OSStatus hotkey_pressed(EventHandlerCallRef call, EventRef event, void *context) {
    (void)call;
    EventHotKeyID key;
    OSStatus status = GetEventParameter(event, kEventParamDirectObject, typeEventHotKeyID,
                                       NULL, sizeof(key), NULL, &key);
    if (status != noErr || key.signature != 'Lnyt') return eventNotHandledErr;
    LennyHotkeys *keys = context;
    if (keys->count < 64) {
        keys->pending[(keys->read + keys->count) % 64] = key.id;
        keys->count++;
    }
    return noErr;
}

void *lenny_macos_hotkeys_new(void) {
    if (![NSThread isMainThread]) return NULL;
    LennyHotkeys *keys = calloc(1, sizeof(*keys));
    if (!keys) return NULL;
    EventTypeSpec type = { kEventClassKeyboard, kEventHotKeyPressed };
    if (InstallEventHandler(GetApplicationEventTarget(), hotkey_pressed, 1, &type,
                            keys, &keys->handler) != noErr) {
        free(keys);
        return NULL;
    }
    return keys;
}

int32_t lenny_macos_hotkey_add(void *context, uint32_t keycode, uint32_t modifiers,
                              uint32_t action) {
    LennyHotkeys *keys = context;
    if (!keys || action >= 64 || keys->refs[action]) return paramErr;
    EventHotKeyID id = { 'Lnyt', action };
    return RegisterEventHotKey(keycode, modifiers, id, GetApplicationEventTarget(),
                               0, &keys->refs[action]);
}

int32_t lenny_macos_hotkey_poll(void *context) {
    LennyHotkeys *keys = context;
    if (!keys || !keys->count) return -1;
    uint32_t action = keys->pending[keys->read];
    keys->read = (keys->read + 1) % 64;
    keys->count--;
    return (int32_t)action;
}

void lenny_macos_hotkeys_free(void *context) {
    LennyHotkeys *keys = context;
    if (!keys) return;
    for (size_t i = 0; i < 64; i++) {
        if (keys->refs[i]) UnregisterEventHotKey(keys->refs[i]);
    }
    RemoveEventHandler(keys->handler);
    free(keys);
}

@interface LennyStatusItem : NSObject
@property NSStatusItem *item;
@property NSMutableArray<NSNumber *> *pending;
- (void)selected:(NSMenuItem *)sender;
@end
@implementation LennyStatusItem
- (void)selected:(NSMenuItem *)sender {
    if (self.pending.count < 64) [self.pending addObject:@(sender.tag)];
}
@end

void *lenny_macos_tray_new(const uint8_t *png, size_t length) {
    if (![NSThread isMainThread]) return NULL;
    LennyStatusItem *tray = [LennyStatusItem new];
    tray.pending = [NSMutableArray new];
    tray.item = [[NSStatusBar systemStatusBar] statusItemWithLength:NSSquareStatusItemLength];
    NSData *data = [NSData dataWithBytes:png length:length];
    NSImage *image = [[NSImage alloc] initWithData:data];
    if (!image) return NULL;
    image.size = NSMakeSize(18, 18);
    image.template = NO;
    image.accessibilityDescription = @"Lennette";
    tray.item.button.image = image;
    tray.item.button.toolTip = @"Lennette";
    NSMenu *menu = [NSMenu new];
    NSArray *titles = @[@"Show Lennette", @"Dashboard", @"Start / stop listening",
                        @"Privacy mode", @"Quit Lennette"];
    for (NSUInteger i = 0; i < titles.count; i++) {
        NSMenuItem *item = [[NSMenuItem alloc] initWithTitle:titles[i]
            action:@selector(selected:) keyEquivalent:@""];
        item.target = tray;
        item.tag = (NSInteger)i;
        [menu addItem:item];
    }
    tray.item.menu = menu;
    return (__bridge_retained void *)tray;
}

int32_t lenny_macos_tray_poll(void *context) {
    if (![NSThread isMainThread]) return -1;
    LennyStatusItem *tray = (__bridge LennyStatusItem *)context;
    if (!tray.pending.count) return -1;
    int32_t result = tray.pending.firstObject.intValue;
    [tray.pending removeObjectAtIndex:0];
    return result;
}

void lenny_macos_tray_tooltip(void *context, const char *text) {
    if (![NSThread isMainThread]) return;
    LennyStatusItem *tray = (__bridge LennyStatusItem *)context;
    tray.item.button.toolTip = [NSString stringWithUTF8String:text];
}

void lenny_macos_tray_free(void *context) {
    LennyStatusItem *tray = (__bridge_transfer LennyStatusItem *)context;
    // TrayBackend is Send; release AppKit resources on the main queue even if
    // a future caller moves its owner to another thread before dropping it.
    void (^remove)(void) = ^{ [[NSStatusBar systemStatusBar] removeStatusItem:tray.item]; };
    if ([NSThread isMainThread]) remove(); else dispatch_async(dispatch_get_main_queue(), remove);
}

#define AUDIO_CAPACITY 32000
@interface LennyAudio : NSObject <SCStreamOutput, SCStreamDelegate> {
@public
    float samples[AUDIO_CAPACITY];
    size_t readIndex, sampleCount;
    uint64_t dropped;
}
@property NSLock *lock;
@property SCStream *stream;
@property dispatch_queue_t queue;
@property NSString *failure;
@property BOOL running;
@property BOOL stopping;
@end

@implementation LennyAudio
- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
    (void)stream;
    [self.lock lock];
    self.running = NO;
    self.failure = error.localizedDescription;
    [self.lock unlock];
}
- (void)stream:(SCStream *)stream didOutputSampleBuffer:(CMSampleBufferRef)sample
       ofType:(SCStreamOutputType)type {
    (void)stream;
    if (type != SCStreamOutputTypeAudio || !CMSampleBufferDataIsReady(sample)) return;
    const AudioStreamBasicDescription *format = CMAudioFormatDescriptionGetStreamBasicDescription(
        CMSampleBufferGetFormatDescription(sample));
    // ScreenCaptureKit is configured for mono 16 kHz float PCM. Refuse format
    // changes rather than reinterpreting bytes and streaming corrupted audio.
    if (!format || format->mFormatID != kAudioFormatLinearPCM ||
        !(format->mFormatFlags & kAudioFormatFlagIsFloat) ||
        format->mBitsPerChannel != 32 || format->mChannelsPerFrame != 1 ||
        format->mSampleRate != 16000) {
        [self.lock lock];
        self.running = NO;
        self.failure = @"ScreenCaptureKit returned an unsupported audio format.";
        [self.lock unlock];
        return;
    }
    AudioBufferList buffers;
    CMBlockBufferRef block = NULL;
    OSStatus status = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(sample, NULL,
        &buffers, sizeof(buffers), kCFAllocatorDefault, kCFAllocatorDefault, 0, &block);
    if (status != noErr) { if (block) CFRelease(block); return; }
    [self.lock lock];
    if (!self.stopping && buffers.mNumberBuffers == 1) {
        const float *data = buffers.mBuffers[0].mData;
        size_t count = buffers.mBuffers[0].mDataByteSize / sizeof(float);
        for (size_t i = 0; data && i < count; i++) {
            if (sampleCount == AUDIO_CAPACITY) {
                readIndex = (readIndex + 1) % AUDIO_CAPACITY;
                sampleCount--;
                dropped++;
            }
            samples[(readIndex + sampleCount) % AUDIO_CAPACITY] = data[i];
            sampleCount++;
        }
    }
    [self.lock unlock];
    if (block) CFRelease(block);
}
@end

// Asynchronous setup keeps the main loop alive during the macOS consent dialog.
// The Rust owner polls status; no synchronous dispatch or permission wait on UI.
void *lenny_macos_audio_new(void) {
    LennyAudio *audio = [LennyAudio new];
    audio.lock = [NSLock new];
    audio.queue = dispatch_queue_create("com.itssingularity.lennette.system-audio", DISPATCH_QUEUE_SERIAL);
    audio.running = YES;
    [SCShareableContent getShareableContentExcludingDesktopWindows:YES onScreenWindowsOnly:YES
        completionHandler:^(SCShareableContent *content, NSError *error) {
        [audio.lock lock];
        if (audio.stopping) { [audio.lock unlock]; return; }
        if (error || !content.displays.count) {
            audio.running = NO;
            audio.failure = error.localizedDescription ?: @"No display is available for system audio.";
            [audio.lock unlock];
            return;
        }
        SCStreamConfiguration *config = [SCStreamConfiguration new];
        config.capturesAudio = YES;
        config.excludesCurrentProcessAudio = YES;
        config.sampleRate = 16000;
        config.channelCount = 1;
        // Audio-only output: no screen frames are delivered or stored.
        config.width = 2;
        config.height = 2;
        config.minimumFrameInterval = CMTimeMake(1, 1);
        SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:content.displays.firstObject
            excludingWindows:@[]];
        audio.stream = [[SCStream alloc] initWithFilter:filter configuration:config delegate:audio];
        NSError *outputError = nil;
        if (![audio.stream addStreamOutput:audio type:SCStreamOutputTypeAudio
                sampleHandlerQueue:audio.queue error:&outputError]) {
            audio.running = NO;
            audio.failure = outputError.localizedDescription;
            audio.stream = nil;
            [audio.lock unlock];
            return;
        }
        [audio.stream startCaptureWithCompletionHandler:^(NSError *startError) {
            [audio.lock lock];
            if (startError && !audio.stopping) {
                audio.running = NO;
                audio.failure = startError.localizedDescription;
            }
            [audio.lock unlock];
        }];
        [audio.lock unlock];
    }];
    return (__bridge_retained void *)audio;
}

size_t lenny_macos_audio_take(void *context, float *output, size_t limit) {
    LennyAudio *audio = (__bridge LennyAudio *)context;
    [audio.lock lock];
    size_t count = MIN(limit, audio->sampleCount);
    for (size_t i = 0; i < count; i++) {
        output[i] = audio->samples[audio->readIndex];
        audio->readIndex = (audio->readIndex + 1) % AUDIO_CAPACITY;
    }
    audio->sampleCount -= count;
    [audio.lock unlock];
    return count;
}

bool lenny_macos_audio_running(void *context) {
    LennyAudio *audio = (__bridge LennyAudio *)context;
    [audio.lock lock];
    bool running = audio.running;
    [audio.lock unlock];
    return running;
}

uint64_t lenny_macos_audio_dropped(void *context) {
    LennyAudio *audio = (__bridge LennyAudio *)context;
    [audio.lock lock];
    uint64_t count = audio->dropped;
    [audio.lock unlock];
    return count;
}

void lenny_macos_audio_failure(void *context, char *output, size_t capacity) {
    LennyAudio *audio = (__bridge LennyAudio *)context;
    [audio.lock lock];
    snprintf(output, capacity, "%s", audio.failure.UTF8String ?: "");
    [audio.lock unlock];
}

void lenny_macos_audio_free(void *context) {
    LennyAudio *audio = (__bridge_transfer LennyAudio *)context;
    [audio.lock lock];
    audio.stopping = YES;
    audio.running = NO;
    SCStream *stream = audio.stream;
    audio.stream = nil;
    [audio.lock unlock];
    if (stream) {
        [stream stopCaptureWithCompletionHandler:^(NSError *error) {
            (void)error;
            [stream removeStreamOutput:audio type:SCStreamOutputTypeAudio error:nil];
        }];
    }
}

@interface LennyScreenshot : NSObject
@property dispatch_semaphore_t ready;
@property NSData *png;
@property NSString *error;
@end
@implementation LennyScreenshot
@end

// Returns -1 on Ventura (the caller uses xcap there), 0 on failure, 1 on success.
// This blocking adapter is used only on screenshot workers, never the UI thread.
// Use Foundation's version check rather than @available: Rust links with
// -nodefaultlibs, which omits clang's __isPlatformVersionAtLeast runtime helper.
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wunguarded-availability-new"
int32_t lenny_macos_screenshot(uint32_t displayID, void **bytes, size_t *length,
                              char *errorText, size_t errorCapacity) {
    if ([[NSProcessInfo processInfo] isOperatingSystemAtLeastVersion:(NSOperatingSystemVersion){14, 0, 0}]) {
        if ([NSThread isMainThread]) {
            snprintf(errorText, errorCapacity, "Screen capture must run off the UI thread.");
            return 0;
        }
        LennyScreenshot *request = [LennyScreenshot new];
        request.ready = dispatch_semaphore_create(0);
        [SCShareableContent getShareableContentExcludingDesktopWindows:YES onScreenWindowsOnly:YES
            completionHandler:^(SCShareableContent *content, NSError *error) {
            SCDisplay *display = nil;
            for (SCDisplay *candidate in content.displays) {
                if (candidate.displayID == displayID) { display = candidate; break; }
            }
            if (error || !display) {
                request.error = error.localizedDescription ?: @"The selected display is no longer available.";
                dispatch_semaphore_signal(request.ready);
                return;
            }
            NSMutableArray *excluded = [NSMutableArray new];
            for (SCRunningApplication *app in content.applications) {
                if (app.processID == getpid()) [excluded addObject:app];
            }
            SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:display
                excludingApplications:excluded exceptingWindows:@[]];
            SCStreamConfiguration *config = [SCStreamConfiguration new];
            // CGDisplayPixelsWide/High preserve Retina physical pixels.
            config.width = CGDisplayPixelsWide(displayID);
            config.height = CGDisplayPixelsHigh(displayID);
            config.showsCursor = NO;
            [SCScreenshotManager captureImageWithFilter:filter configuration:config
                completionHandler:^(CGImageRef image, NSError *captureError) {
                if (image) {
                    NSBitmapImageRep *bitmap = [[NSBitmapImageRep alloc] initWithCGImage:image];
                    request.png = [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
                }
                request.error = captureError.localizedDescription;
                dispatch_semaphore_signal(request.ready);
            }];
        }];
        if (dispatch_semaphore_wait(request.ready, dispatch_time(DISPATCH_TIME_NOW, 15 * NSEC_PER_SEC))) {
            snprintf(errorText, errorCapacity, "Screen capture timed out. Allow Screen Recording in System Settings, then try again.");
            return 0;
        }
        if (!request.png.length) {
            snprintf(errorText, errorCapacity, "%s", request.error.UTF8String ?: "Screen capture returned no image.");
            return 0;
        }
        *length = request.png.length;
        *bytes = malloc(*length);
        if (!*bytes) { snprintf(errorText, errorCapacity, "Could not allocate the screenshot."); return 0; }
        memcpy(*bytes, request.png.bytes, *length);
        return 1;
    }
    return -1;
}

#pragma clang diagnostic pop

void lenny_macos_bytes_free(void *bytes) { free(bytes); }
