#import <AppKit/AppKit.h>
#import <CoreServices/CoreServices.h>

static void record(NSString *target) {
    NSData *bytes = [target dataUsingEncoding:NSUTF8StringEncoding];
    NSMutableString *line = [NSMutableString new];
    const unsigned char *raw = bytes.bytes;
    for (NSUInteger i = 0; i < bytes.length; i++) [line appendFormat:@"%02x", raw[i]];
    [line appendString:@"\n"];
    NSFileHandle *file = [NSFileHandle fileHandleForWritingAtPath:@SODECK_OPEN_HANDLER_LOG];
    [file seekToEndOfFile]; [file writeData:[line dataUsingEncoding:NSUTF8StringEncoding]]; [file closeFile];
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 200 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{ [NSApp terminate:nil]; });
}

@interface Handler : NSObject <NSApplicationDelegate>
@end
@implementation Handler
- (void)application:(NSApplication *)app openURLs:(NSArray<NSURL *> *)urls {
    for (NSURL *url in urls) record(url.isFileURL ? url.path : url.absoluteString);
}
- (void)application:(NSApplication *)app openFiles:(NSArray<NSString *> *)files {
    for (NSString *file in files) record(file);
    [app replyToOpenOrPrint:NSApplicationDelegateReplySuccess];
}
@end

int main(int argc, const char **argv) {
    @autoreleasepool {
        if (argc > 1 && strcmp(argv[1], "associations") == 0) {
            NSArray *old = @[(id)CFBridgingRelease(LSCopyDefaultHandlerForURLScheme(CFSTR("http"))) ?: @"",
                (id)CFBridgingRelease(LSCopyDefaultHandlerForURLScheme(CFSTR("https"))) ?: @"",
                (id)CFBridgingRelease(LSCopyDefaultRoleHandlerForContentType(CFSTR("public.folder"), kLSRolesViewer)) ?: @"com.apple.finder"];
            puts([[NSString alloc] initWithData:[NSJSONSerialization dataWithJSONObject:old options:0 error:nil] encoding:NSUTF8StringEncoding].UTF8String);
            return 0;
        }
        if (argc > 1 && strcmp(argv[1], "set") == 0) {
            OSStatus http = argc > 2 && strlen(argv[2]) ? LSSetDefaultHandlerForURLScheme(CFSTR("http"), (__bridge CFStringRef)[NSString stringWithUTF8String:argv[2]]) : noErr;
            OSStatus https = argc > 3 && strlen(argv[3]) ? LSSetDefaultHandlerForURLScheme(CFSTR("https"), (__bridge CFStringRef)[NSString stringWithUTF8String:argv[3]]) : noErr;
            OSStatus folder = LSSetDefaultRoleHandlerForContentType(CFSTR("public.folder"), kLSRolesViewer, (__bridge CFStringRef)[NSString stringWithUTF8String:argv[4]]);
            if (http || https || folder) fprintf(stderr, "LaunchServices defaults: http=%d https=%d folder=%d\n", (int)http, (int)https, (int)folder);
            return http == noErr && https == noErr && folder == noErr ? 0 : 1;
        }
        if (argc > 1 && strcmp(argv[1], "register") == 0) {
            return LSRegisterURL((__bridge CFURLRef)[NSURL fileURLWithPath:[NSString stringWithUTF8String:argv[2]]], true) == noErr ? 0 : 1;
        }
        NSApplication *app = [NSApplication sharedApplication];
        Handler *delegate = [Handler new]; app.delegate = delegate; [app run];
    }
    return 0;
}
