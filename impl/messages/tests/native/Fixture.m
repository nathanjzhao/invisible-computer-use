// Same native delivery code, addressed only to this in-process synthetic scripting application.
#define CUA_MESSAGES_TARGET "com.nathanzhao.fixture.Messages"
#define CUA_FIXTURE_SELF 1
#define main production_helper_main
#include "../../native/MessagesBridge.m"
#undef main

@class FixtureApplication,FixtureAccount;
@interface FixtureParticipant:NSObject
@property NSString *identifier;@property NSString *handle;@property(weak) FixtureAccount *owner;
@end
@interface FixtureChat:NSObject
@property NSString *identifier;@property NSString *name;@property FixtureAccount *account;@property NSMutableArray *participants;
@end
@interface FixtureAccount:NSObject
@property NSString *identifier;@property BOOL enabled;@property OSType connectionStatus;@property OSType serviceType;@property NSMutableArray *participants;@property NSMutableArray *chats;
@end
@interface FixtureApplication:NSApplication
@property NSMutableArray *accounts;@property NSMutableArray *chats;@property NSMutableArray *captured;@property BOOL failSend;
@property NSString *cancelOnSend;
- (void)handleMake:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)reply;
@end
static FixtureApplication *fixture;
static NSString *captureFile;
static NSScriptObjectSpecifier *unique(Class cls,NSScriptObjectSpecifier *container,NSString *key,NSString *identifier){NSScriptClassDescription *description=container?(NSScriptClassDescription *)container.keyClassDescription:(NSScriptClassDescription *)[NSScriptClassDescription classDescriptionForClass:FixtureApplication.class];return [[NSUniqueIDSpecifier alloc] initWithContainerClassDescription:description containerSpecifier:container key:key uniqueID:identifier];}
@implementation FixtureParticipant
- (NSScriptObjectSpecifier *)objectSpecifier{return unique(self.class,self.owner.objectSpecifier,@"participants",self.identifier);}
@end
@implementation FixtureChat
- (instancetype)init{self=[super init];if(self){_identifier=NSUUID.UUID.UUIDString;_name=@"Fixture created group";_participants=[NSMutableArray array];}return self;}
- (NSScriptObjectSpecifier *)objectSpecifier{return unique(self.class,nil,@"chats",self.identifier);}
@end
@implementation FixtureAccount
- (NSScriptObjectSpecifier *)objectSpecifier{return unique(self.class,nil,@"accounts",self.identifier);}
- (void)insertObject:(FixtureChat *)chat inChatsAtIndex:(NSUInteger)index{chat.account=self;[self.chats insertObject:chat atIndex:index];[fixture.chats addObject:chat];}
@end
@implementation FixtureApplication
- (void)handleMake:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)reply{
    fprintf(stderr,"Fixture received create event\n");
    NSAppleEventDescriptor *props=[event paramDescriptorForKeyword:keyAEPropData];NSAppleEventDescriptor *peers=[props descriptorForKeyword:'pres'];
    if([[event paramDescriptorForKeyword:keyAEObjectClass] typeCodeValue]!='imct'||peers.numberOfItems<2){[reply setParamDescriptor:[NSAppleEventDescriptor descriptorWithInt32:-1700] forKeyword:keyErrorNumber];return;}
    FixtureAccount *account=self.accounts[0];NSMutableArray *people=[NSMutableArray array];
    for(NSInteger i=1;i<=peers.numberOfItems;i++){NSString *identifier=[[[peers descriptorAtIndex:i] descriptorForKeyword:keyAEKeyData] stringValue];FixtureParticipant *found=nil;for(FixtureParticipant *person in account.participants)if([person.identifier isEqual:identifier]){found=person;break;}if(!found){[reply setParamDescriptor:[NSAppleEventDescriptor descriptorWithInt32:-1728] forKeyword:keyErrorNumber];return;}[people addObject:found];}
    FixtureChat *chat=[FixtureChat new];chat.account=account;chat.participants=people;[account.chats addObject:chat];[self.chats addObject:chat];[reply setParamDescriptor:chat.objectSpecifier.descriptor forKeyword:keyDirectObject];
}
@end
@interface FixtureSendCommand:NSScriptCommand @end
@implementation FixtureSendCommand
- (id)performDefaultImplementation{
    id payload=self.directParameter;id target=self.evaluatedArguments[@"to"];NSString *destination=[target valueForKey:@"identifier"];
    NSMutableDictionary *record=[@{@"destination":destination?:@"missing"} mutableCopy];
    if([payload isKindOfClass:NSString.class])record[@"text"]=payload;
    else if([payload isKindOfClass:NSURL.class]){NSData *data=[NSData dataWithContentsOfURL:payload];record[@"file_bytes"]=data?[data base64EncodedStringWithOptions:0]:@"missing";}
    else record[@"payload_class"]=NSStringFromClass([payload class]);
    [fixture.captured addObject:record];
    if(captureFile){NSData *line=[NSJSONSerialization dataWithJSONObject:record options:0 error:nil];NSFileHandle *handle=[NSFileHandle fileHandleForWritingAtPath:captureFile];if(!handle){[NSFileManager.defaultManager createFileAtPath:captureFile contents:nil attributes:@{NSFilePosixPermissions:@0600}];handle=[NSFileHandle fileHandleForWritingAtPath:captureFile];}[handle seekToEndOfFile];[handle writeData:line];[handle writeData:[@"\n" dataUsingEncoding:NSUTF8StringEncoding]];[handle synchronizeFile];[handle closeFile];}
    if(fixture.cancelOnSend)[NSFileManager.defaultManager removeItemAtPath:fixture.cancelOnSend error:nil];
    if(fixture.failSend){self.scriptErrorNumber=-1708;self.scriptErrorString=@"Fixture lost acknowledgement";}
    return nil;
}
@end
static void setup(void){
    fixture.accounts=[NSMutableArray array];fixture.chats=[NSMutableArray array];fixture.captured=[NSMutableArray array];
    FixtureAccount *account=[FixtureAccount new];account.identifier=@"fixture-account";account.enabled=YES;account.connectionStatus='conn';account.serviceType='sims';account.participants=[NSMutableArray array];account.chats=[NSMutableArray array];[fixture.accounts addObject:account];
    for(NSString *handle in @[@"one@example.invalid",@"two@example.invalid"]){FixtureParticipant *person=[FixtureParticipant new];person.identifier=[NSString stringWithFormat:@"%@:%@",account.identifier,handle];person.handle=handle;person.owner=account;[account.participants addObject:person];}
    FixtureChat *chat=[FixtureChat new];chat.identifier=@"fixture-direct";chat.name=@"Fixture direct";chat.account=account;chat.participants=[NSMutableArray arrayWithObject:account.participants[0]];[account.chats addObject:chat];[fixture.chats addObject:chat];
}
int main(int argc,const char *argv[]){@autoreleasepool{
    BOOL scenario=argc>=2&&argv[1][0]=='/';NSString *receipt=scenario?[NSString stringWithUTF8String:argv[1]]:nil;NSDictionary *request=nil;
    if(scenario)[@"{\"phase\":\"starting\"}" writeToFile:receipt atomically:YES encoding:NSUTF8StringEncoding error:nil];
    else {NSData *input=[NSFileHandle.fileHandleWithStandardInput readDataToEndOfFile];if(input.length>1024*1024)return 2;request=[NSJSONSerialization JSONObjectWithData:input options:0 error:nil];if(![request isKindOfClass:NSDictionary.class])return 2;captureFile=[NSBundle.mainBundle.bundleURL.URLByDeletingLastPathComponent.path stringByAppendingPathComponent:@"native-fixture-capture.jsonl"];}
    fixture=[FixtureApplication sharedApplication];[fixture setActivationPolicy:NSApplicationActivationPolicyProhibited];setup();[fixture finishLaunching];
    [NSAppleEventManager.sharedAppleEventManager setEventHandler:fixture andSelector:@selector(handleMake:withReplyEvent:) forEventClass:kAECoreSuite andEventID:kAECreateElement];
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW,35*NSEC_PER_SEC),dispatch_get_global_queue(QOS_CLASS_UTILITY,0),^{_exit(4);});
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW,300*NSEC_PER_MSEC),dispatch_get_global_queue(QOS_CLASS_USER_INITIATED,0),^{@autoreleasepool{
        if(!scenario){
            BOOL warm=NO;(void)run(@{@"operation":@"status"},&warm);
            dispatch_sync(dispatch_get_main_queue(),^{[NSAppleEventManager.sharedAppleEventManager setEventHandler:fixture andSelector:@selector(handleMake:withReplyEvent:) forEventClass:kAECoreSuite andEventID:kAECreateElement];});
            if(request[@"nonce"])emit(@{@"event":@"started",@"nonce":request[@"nonce"],@"pid":@(getpid())});
            if([request[@"text"] isEqual:@"fixture force uncertain"])fixture.failSend=YES;
            BOOL dispatched=NO;emit(run(request,&dispatched));dispatch_async(dispatch_get_main_queue(),^{[fixture terminate:nil];});return;
        }
        NSMutableArray *results=[NSMutableArray array];BOOL sent=NO;
        [@"{\"phase\":\"status\"}" writeToFile:receipt atomically:YES encoding:NSUTF8StringEncoding error:nil];
        NSDictionary *existing=@{@"kind":@"existing",@"chat_guid":@"fixture-direct",@"service":@"iMessage",@"recipients":@[@"one@example.invalid"]};
        [results addObject:run(@{@"operation":@"status"},&sent)];
        [[NSJSONSerialization dataWithJSONObject:@{@"phase":@"existing-send",@"results":results} options:0 error:nil] writeToFile:receipt atomically:YES];
        [results addObject:run(@{@"operation":@"send",@"target":existing,@"text":@"literal \"quotes\"; do shell script is data\n🐾",@"files":@[]},&sent)];
        NSDictionary *newTarget=@{@"kind":@"new",@"recipients":@[@"two@example.invalid"]};
        [results addObject:run(@{@"operation":@"send",@"target":newTarget,@"text":@"new direct",@"files":@[]},&sent)];
        NSDictionary *group=@{@"kind":@"new",@"recipients":@[@"one@example.invalid",@"two@example.invalid"]};
        dispatch_sync(dispatch_get_main_queue(),^{[NSAppleEventManager.sharedAppleEventManager setEventHandler:fixture andSelector:@selector(handleMake:withReplyEvent:) forEventClass:kAECoreSuite andEventID:kAECreateElement];});
        [results addObject:run(@{@"operation":@"send",@"target":group,@"text":@"new group",@"files":@[]},&sent)];
        NSString *filePath=[receipt.stringByDeletingLastPathComponent stringByAppendingPathComponent:@"fixture-file.txt"];NSData *fileData=[@"native attachment fixture 🐾" dataUsingEncoding:NSUTF8StringEncoding];[fileData writeToFile:filePath atomically:YES];unsigned char hash[CC_SHA256_DIGEST_LENGTH];CC_SHA256(fileData.bytes,(CC_LONG)fileData.length,hash);NSMutableString *digest=[NSMutableString string];for(int i=0;i<sizeof(hash);i++)[digest appendFormat:@"%02x",hash[i]];
        NSDictionary *file=@{@"staged":filePath,@"size":@(fileData.length),@"sha256":digest};
        [results addObject:run(@{@"operation":@"send",@"target":existing,@"text":NSNull.null,@"files":@[file]},&sent)];
        NSDictionary *mismatch=@{@"kind":@"existing",@"chat_guid":@"fixture-direct",@"service":@"iMessage",@"recipients":@[@"two@example.invalid"]};
        [results addObject:run(@{@"operation":@"send",@"target":mismatch,@"text":@"must not be captured",@"files":@[]},&sent)];
        fixture.failSend=YES;[results addObject:run(@{@"operation":@"send",@"target":existing,@"text":@"uncertain fixture",@"files":@[]},&sent)];fixture.failSend=NO;
        [results addObject:run(@{@"operation":@"send",@"target":existing,@"text":@"dead parent must not dispatch",@"files":@[],@"parent_pid":@(getpid()),@"parent_started":@[@0,@0]},&sent)];
        NSString *lifetime=[receipt.stringByDeletingLastPathComponent stringByAppendingPathComponent:@"fixture-alive"];[@"alive" writeToFile:lifetime atomically:YES encoding:NSUTF8StringEncoding error:nil];fixture.cancelOnSend=lifetime;
        [results addObject:run(@{@"operation":@"send",@"target":existing,@"text":@"partial fixture",@"files":@[file],@"lifetime":lifetime},&sent)];fixture.cancelOnSend=nil;
        ((FixtureAccount *)fixture.accounts[0]).connectionStatus='dcon';[results addObject:run(@{@"operation":@"send",@"target":existing,@"text":@"offline must not dispatch",@"files":@[]},&sent)];
        NSData *manifestData=[NSData dataWithContentsOfURL:[NSBundle.mainBundle URLForResource:@"build-manifest" withExtension:@"json"]];NSDictionary *manifest=manifestData?[NSJSONSerialization JSONObjectWithData:manifestData options:0 error:nil]:@{};
        NSDictionary *output=@{@"results":results,@"captured":fixture.captured,@"chat_count":@(fixture.chats.count),@"native_messages_sent":@0,@"fixture_only":@YES,@"tcc_tested":@NO,@"source_manifest":manifest};NSData *data=[NSJSONSerialization dataWithJSONObject:output options:NSJSONWritingPrettyPrinted error:nil];[data writeToFile:receipt atomically:YES];dispatch_async(dispatch_get_main_queue(),^{[fixture terminate:nil];});
    }});
    [fixture run];return 0;
}}
