// Runs the real bridge against AppKit/Carbon, without requesting recording or
// Accessibility permission. Build and execute via scripts/macos-smoke.sh.
#import <AppKit/AppKit.h>
#import <Carbon/Carbon.h>
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
// Compile the production bridge in this test translation unit so the audio
// callback can be exercised with real CoreMedia buffers without recording.
#import "../src/platform/macos/native.m"

static void check_audio_ring(void) {
    LennyAudio *audio = [LennyAudio new];
    audio.lock = [NSLock new];
    audio.running = YES;
    const size_t count = 40000;
    AudioStreamBasicDescription format = {0};
    format.mSampleRate = 16000;
    format.mFormatID = kAudioFormatLinearPCM;
    format.mFormatFlags = kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked;
    format.mBytesPerPacket = sizeof(float);
    format.mFramesPerPacket = 1;
    format.mBytesPerFrame = sizeof(float);
    format.mChannelsPerFrame = 1;
    format.mBitsPerChannel = 32;
    CMAudioFormatDescriptionRef description = NULL;
    assert(CMAudioFormatDescriptionCreate(kCFAllocatorDefault, &format, 0, NULL,
                                         0, NULL, NULL, &description) == noErr);
    CMBlockBufferRef block = NULL;
    assert(CMBlockBufferCreateWithMemoryBlock(kCFAllocatorDefault, NULL, count * sizeof(float),
        kCFAllocatorDefault, NULL, 0, count * sizeof(float), 0, &block) == noErr);
    float *input = calloc(count, sizeof(float));
    assert(input);
    for (size_t i = 0; i < count; i++) input[i] = (float)i / count;
    assert(CMBlockBufferReplaceDataBytes(input, block, 0, count * sizeof(float)) == noErr);
    free(input);
    CMSampleBufferRef sample = NULL;
    assert(CMAudioSampleBufferCreateReadyWithPacketDescriptions(kCFAllocatorDefault,
        block, description, (CMItemCount)count, kCMTimeZero, NULL, &sample) == noErr);
    SCStream *unusedStream = nil; // Callback does not dereference its stream parameter.
    [audio stream:unusedStream didOutputSampleBuffer:sample ofType:SCStreamOutputTypeAudio];
    void *context = (__bridge void *)audio;
    assert(lenny_macos_audio_running(context));
    assert(lenny_macos_audio_dropped(context) == count - AUDIO_CAPACITY);
    float output[3] = {0};
    assert(lenny_macos_audio_take(context, output, 3) == 3);
    assert(output[0] == (float)(count - AUDIO_CAPACITY) / count);
    assert(output[2] == (float)(count - AUDIO_CAPACITY + 2) / count);
    char failure[256];
    lenny_macos_audio_failure(context, failure, sizeof(failure));
    assert(!failure[0]);
    NSError *error = [NSError errorWithDomain:@"LennetteSmoke" code:1
        userInfo:@{NSLocalizedDescriptionKey: @"Recording permission revoked"}];
    [audio stream:unusedStream didStopWithError:error];
    assert(!lenny_macos_audio_running(context));
    lenny_macos_audio_failure(context, failure, sizeof(failure));
    assert(strcmp(failure, "Recording permission revoked") == 0);
    CFRelease(sample);
    CFRelease(block);
    CFRelease(description);
}

int main(void) {
    @autoreleasepool {
        check_audio_ring();
        [NSApplication sharedApplication];
        NSWindow *window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 160, 100)
            styleMask:(NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable | NSWindowStyleMaskResizable) backing:NSBackingStoreBuffered defer:NO];
        void *view = (__bridge void *)window.contentView;
        assert(lenny_macos_prepare_window(view));
        assert([window standardWindowButton:NSWindowCloseButton].hidden);
        assert([window standardWindowButton:NSWindowMiniaturizeButton].hidden);
        assert([window standardWindowButton:NSWindowZoomButton].hidden);
        assert(window.titleVisibility == NSWindowTitleHidden);
        assert(window.styleMask & NSWindowStyleMaskFullSizeContentView);
        assert(window.collectionBehavior & NSWindowCollectionBehaviorFullScreenAuxiliary);
        assert(window.collectionBehavior & NSWindowCollectionBehaviorCanJoinAllSpaces);
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        assert(window.sharingType == NSWindowSharingNone);
#pragma clang diagnostic pop
        // Verifies the setting was applied, not that a third-party recorder honors it.
        assert(lenny_macos_set_above(view, true));
        assert(window.level == NSFloatingWindowLevel);
        assert(lenny_macos_set_above(view, false));
        assert(window.level == NSNormalWindowLevel);
        assert(lenny_macos_set_icon_hidden(true));
        assert(NSApp.activationPolicy == NSApplicationActivationPolicyAccessory);
        assert(lenny_macos_set_icon_hidden(false));
        assert(NSApp.activationPolicy == NSApplicationActivationPolicyRegular);
        void *keys = lenny_macos_hotkeys_new();
        assert(keys);
        // Use Control+Option+Shift+F19 to avoid the running app's defaults.
        assert(lenny_macos_hotkey_add(keys, kVK_F19, controlKey | optionKey | shiftKey, 0) == noErr);
        assert(lenny_macos_hotkey_add(keys, kVK_F19, controlKey | optionKey | shiftKey, 1) != noErr);
        assert(lenny_macos_hotkey_poll(keys) == -1);
        EventRef event = NULL;
        EventHotKeyID id = { 'Lnyt', 0 };
        assert(CreateEvent(NULL, kEventClassKeyboard, kEventHotKeyPressed,
                           GetCurrentEventTime(), kEventAttributeUserEvent, &event) == noErr);
        assert(SetEventParameter(event, kEventParamDirectObject, typeEventHotKeyID, sizeof(id), &id) == noErr);
        assert(SendEventToEventTarget(event, GetApplicationEventTarget()) == noErr);
        ReleaseEvent(event);
        assert(lenny_macos_hotkey_poll(keys) == 0);
        assert(lenny_macos_hotkey_poll(keys) == -1);
        lenny_macos_hotkeys_free(keys);
        // Cleanup must release registrations, allowing the same shortcut again.
        keys = lenny_macos_hotkeys_new();
        assert(keys);
        assert(lenny_macos_hotkey_add(keys, kVK_F19, controlKey | optionKey | shiftKey, 0) == noErr);
        lenny_macos_hotkeys_free(keys);
        assert(lenny_macos_tray_new(NULL, 0) == NULL);
        NSData *icon = [NSData dataWithContentsOfFile:@"src/assets/lennette_app_icon.png"];
        assert(icon.length);
        void *tray = lenny_macos_tray_new(icon.bytes, icon.length);
        assert(tray);
        LennyStatusItem *statusItem = (__bridge LennyStatusItem *)tray;
        assert(statusItem.item.button.image);
        assert(!statusItem.item.button.image.isTemplate);
        assert(statusItem.item.button.image.size.width == 18);
        assert(lenny_macos_tray_poll(tray) == -1);
        lenny_macos_tray_tooltip(tray, "Lennette native smoke test");
        lenny_macos_tray_free(tray);
        // NSWindow's default close releases itself under manual reference counting;
        // ARC still owns it here, so avoid combining close with automatic release.
        window.releasedWhenClosed = NO;
        [window close];
        puts("macOS native smoke passed: window policy, Spaces, stacking, Dock, hotkey dispatch/conflicts/cleanup, menu bar, CoreMedia PCM buffering and capture failure propagation");
    }
}
