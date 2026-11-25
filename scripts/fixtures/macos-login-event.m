#import <AppKit/AppKit.h>
#import <Carbon/Carbon.h>

// Test launcher: exercise the system's Apple Event delivery, not a fabricated Rust flag.
int main(int argc, const char **argv) {
  @autoreleasepool {
    if (argc < 2) return 2;
    NSString *binary = [NSString stringWithUTF8String:argv[1]];
    NSURL *bundle = [NSURL fileURLWithPath:[[[binary stringByDeletingLastPathComponent] stringByDeletingLastPathComponent] stringByDeletingLastPathComponent]];
    NSWorkspaceOpenConfiguration *config = [NSWorkspaceOpenConfiguration configuration];
    NSMutableArray *arguments = [NSMutableArray new];
    for (int i = 2; i < argc; ++i) [arguments addObject:[NSString stringWithUTF8String:argv[i]]];
    config.arguments = arguments;
    config.environment = NSProcessInfo.processInfo.environment;
    config.activates = NO;
    config.createsNewApplicationInstance = YES;
    NSAppleEventDescriptor *event = [NSAppleEventDescriptor appleEventWithEventClass:kCoreEventClass eventID:kAEOpenApplication targetDescriptor:nil returnID:kAutoGenerateReturnID transactionID:kAnyTransactionID];
    [event setParamDescriptor:[NSAppleEventDescriptor descriptorWithEnumCode:keyAELaunchedAsLogInItem] forKeyword:keyAEPropData];
    config.appleEvent = event;
    __block BOOL finished = NO;
    __block NSRunningApplication *running = nil;
    __block NSError *failure = nil;
    [[NSWorkspace sharedWorkspace] openApplicationAtURL:bundle configuration:config completionHandler:^(NSRunningApplication *app, NSError *error) { running = app; failure = error; finished = YES; }];
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:90];
    while ((!finished || (running && !running.terminated)) && deadline.timeIntervalSinceNow > 0)
      [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.05]];
    if (failure) { fprintf(stderr, "%s\n", failure.localizedDescription.UTF8String); return 1; }
    return finished && running && running.terminated ? 0 : 1;
  }
}
