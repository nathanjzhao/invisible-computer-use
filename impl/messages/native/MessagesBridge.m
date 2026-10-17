#import "MessagesScripting.h"
#import <Carbon/Carbon.h>
#import <CommonCrypto/CommonDigest.h>
#import <sys/resource.h>
#import <sys/stat.h>
#import <fcntl.h>
#import <unistd.h>
#import <libproc.h>

#ifndef CUA_MESSAGES_TARGET
#define CUA_MESSAGES_TARGET "com.apple.MobileSMS"
#endif

static NSDictionary *failure(NSString *code,NSString *message){return @{ @"ok":@NO,@"code":code,@"message":message };}
static NSDictionary *success(NSDictionary *data){return @{ @"ok":@YES,@"data":data };}
static void emit(NSDictionary *object){NSData *bytes=[NSJSONSerialization dataWithJSONObject:object options:NSJSONWritingSortedKeys error:nil];if(bytes.length>2*1024*1024)_exit(3);[[NSFileHandle fileHandleWithStandardOutput] writeData:bytes];[[NSFileHandle fileHandleWithStandardOutput] writeData:[@"\n" dataUsingEncoding:NSUTF8StringEncoding]];}
static NSString *normalizeAddress(NSString *address){NSString *s=[address stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];if([s containsString:@"@"])return s.lowercaseString;NSCharacterSet *allowed=[NSCharacterSet characterSetWithCharactersInString:@"0123456789+()- ."];if([s rangeOfCharacterFromSet:allowed.invertedSet].location!=NSNotFound)return s.lowercaseString;return [[s componentsSeparatedByCharactersInSet:[NSCharacterSet characterSetWithCharactersInString:@"0123456789"].invertedSet] componentsJoinedByString:@""];}
static NSSet *members(NSArray *values){NSMutableSet *result=[NSMutableSet set];for(id value in values){if(![value isKindOfClass:NSString.class])return nil;NSString *n=normalizeAddress(value);if(!n.length)return nil;[result addObject:n];}return result;}
static NSString *service(OSType kind){switch(kind){case 'sims':return @"iMessage";case 'ssms':return @"SMS";case 'srcs':return @"RCS";default:return @"unknown";}}
@interface FailureObserver:NSObject<SBApplicationDelegate>
@property(nonatomic,strong) NSError *error;
@end
@implementation FailureObserver
- (id)eventDidFail:(const AppleEvent *)event withError:(NSError *)error{self.error=error;return nil;}
@end

static NSDictionary *checkFile(NSDictionary *file){
    NSString *path=file[@"staged"];NSString *digest=file[@"sha256"];NSNumber *expected=file[@"size"];
    if(![path isKindOfClass:NSString.class]||![path isAbsolutePath]||![digest isKindOfClass:NSString.class]||![expected isKindOfClass:NSNumber.class])return failure(@"messagesSendPlanChanged",@"Invalid staged attachment");
    int fd=open(path.fileSystemRepresentation,O_RDONLY|O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC);if(fd<0)return failure(@"messagesSendPlanChanged",@"A staged attachment could not be opened");
    struct stat before,after;if(fstat(fd,&before)!=0||!S_ISREG(before.st_mode)||before.st_size!=expected.unsignedLongLongValue){close(fd);return failure(@"messagesSendPlanChanged",@"A staged attachment changed");}
    CC_SHA256_CTX hash;CC_SHA256_Init(&hash);unsigned char buffer[65536];ssize_t n;unsigned long long size=0;
    while((n=read(fd,buffer,sizeof(buffer)))>0){size+=n;if(size>expected.unsignedLongLongValue)break;CC_SHA256_Update(&hash,buffer,(CC_LONG)n);}
    BOOL stable=n==0&&size==expected.unsignedLongLongValue&&fstat(fd,&after)==0&&before.st_ino==after.st_ino&&before.st_size==after.st_size&&before.st_mtimespec.tv_sec==after.st_mtimespec.tv_sec&&before.st_mtimespec.tv_nsec==after.st_mtimespec.tv_nsec;close(fd);
    unsigned char bytes[CC_SHA256_DIGEST_LENGTH];CC_SHA256_Final(bytes,&hash);NSMutableString *actual=[NSMutableString string];for(int i=0;i<CC_SHA256_DIGEST_LENGTH;i++)[actual appendFormat:@"%02x",bytes[i]];
    return stable&&[actual isEqualToString:digest]?nil:failure(@"messagesSendPlanChanged",@"A staged attachment changed during validation");
}
static BOOL alive(NSDictionary *request){
    NSString *path=request[@"lifetime"];if(path&&![[NSFileManager defaultManager] fileExistsAtPath:path])return NO;
    NSNumber *parent=request[@"parent_pid"];NSArray *started=request[@"parent_started"];
    if(parent){struct proc_bsdinfo info;int length=proc_pidinfo(parent.intValue,PROC_PIDTBSDINFO,0,&info,sizeof(info));if(length!=sizeof(info)||started.count!=2||info.pbi_start_tvsec!=[started[0] unsignedLongLongValue]||info.pbi_start_tvusec!=[started[1] unsignedLongLongValue])return NO;}
    return YES;
}
static OSStatus permission(BOOL ask){
#ifdef CUA_FIXTURE_SELF
    // The in-process scripting fixture tests serialization/dispatch, not TCC policy.
    return noErr;
#else
    NSAppleEventDescriptor *target=[NSAppleEventDescriptor descriptorWithBundleIdentifier:@CUA_MESSAGES_TARGET];return AEDeterminePermissionToAutomateTarget(target.aeDesc,'icht','send',ask);
#endif
}
static NSArray *connected(CUMApplication *app,FailureObserver *observer){NSArray *all=[[app accounts] get];if(observer.error||![all isKindOfClass:NSArray.class])return nil;NSMutableArray *result=[NSMutableArray array];for(CUMAccount *account in all){if(account.enabled&&account.connectionStatus=='conn')[result addObject:account];if(observer.error)return nil;}return result;}
static NSAppleEventDescriptor *specifier(OSType desired,NSAppleEventDescriptor *container,OSType form,NSAppleEventDescriptor *key){
    NSAppleEventDescriptor *record=NSAppleEventDescriptor.recordDescriptor;
    [record setDescriptor:[NSAppleEventDescriptor descriptorWithTypeCode:desired] forKeyword:keyAEDesiredClass];
    [record setDescriptor:container?:NSAppleEventDescriptor.nullDescriptor forKeyword:keyAEContainer];
    [record setDescriptor:[NSAppleEventDescriptor descriptorWithEnumCode:form] forKeyword:keyAEKeyForm];[record setDescriptor:key forKeyword:keyAEKeyData];
    return [record coerceToDescriptorType:typeObjectSpecifier];
}
static NSString *createGroup(NSString *accountID,NSArray *participantIDs,NSError **error){
    NSAppleEventDescriptor *account=specifier('icsv',nil,formUniqueID,[NSAppleEventDescriptor descriptorWithString:accountID]);
    NSAppleEventDescriptor *peers=NSAppleEventDescriptor.listDescriptor;
    for(NSString *identifier in participantIDs){[peers insertDescriptor:specifier('pres',account,formUniqueID,[NSAppleEventDescriptor descriptorWithString:identifier]) atIndex:0];}
    NSAppleEventDescriptor *properties=NSAppleEventDescriptor.recordDescriptor;[properties setDescriptor:peers forKeyword:'pres'];
    NSAppleEventDescriptor *app=[NSAppleEventDescriptor descriptorWithBundleIdentifier:@CUA_MESSAGES_TARGET];
    NSAppleEventDescriptor *make=[NSAppleEventDescriptor appleEventWithEventClass:kAECoreSuite eventID:kAECreateElement targetDescriptor:app returnID:kAutoGenerateReturnID transactionID:kAnyTransactionID];
    [make setParamDescriptor:[NSAppleEventDescriptor descriptorWithTypeCode:'imct'] forKeyword:keyAEObjectClass];[make setParamDescriptor:account forKeyword:keyAEInsertHere];[make setParamDescriptor:properties forKeyword:keyAEPropData];
    NSAppleEventDescriptor *reply=[make sendEventWithOptions:NSAppleEventSendWaitForReply|NSAppleEventSendNeverInteract timeout:10 error:error];
    SInt32 creationCode=[[reply paramDescriptorForKeyword:keyErrorNumber] int32Value];
    if(creationCode){if(error)*error=[NSError errorWithDomain:NSOSStatusErrorDomain code:creationCode userInfo:@{NSLocalizedDescriptionKey:[[reply paramDescriptorForKeyword:keyErrorString] stringValue]?:@"Native group creation failed"}];return nil;}
    NSAppleEventDescriptor *created=[reply paramDescriptorForKeyword:keyDirectObject];if(!created)return nil;
    NSAppleEventDescriptor *get=[NSAppleEventDescriptor appleEventWithEventClass:kAECoreSuite eventID:kAEGetData targetDescriptor:app returnID:kAutoGenerateReturnID transactionID:kAnyTransactionID];
    [get setParamDescriptor:specifier(typeProperty,created,formPropertyID,[NSAppleEventDescriptor descriptorWithTypeCode:'ID  ']) forKeyword:keyDirectObject];
    NSAppleEventDescriptor *identity=[get sendEventWithOptions:NSAppleEventSendWaitForReply|NSAppleEventSendNeverInteract timeout:10 error:error];return [[identity paramDescriptorForKeyword:keyDirectObject] stringValue];
}
static NSDictionary *run(NSDictionary *request,BOOL *dispatched){
    *dispatched=NO;
    NSString *operation=request[@"operation"];
    if(![@[@"status",@"authorize",@"preflight",@"send"] containsObject:operation])return failure(@"invalidArguments",@"Unknown Messages helper operation");
    CUMApplication *app=(CUMApplication *)[SBApplication applicationWithBundleIdentifier:@CUA_MESSAGES_TARGET];
    if(!app)return failure(@"messagesApplicationUnavailable",@"The Messages application is unavailable");
    if([operation isEqual:@"authorize"]&&![app isRunning]){
        NSURL *url=[[NSWorkspace sharedWorkspace] URLForApplicationWithBundleIdentifier:@CUA_MESSAGES_TARGET];if(!url)return failure(@"messagesApplicationUnavailable",@"The Messages application is unavailable");
        NSWorkspaceOpenConfiguration *config=NSWorkspaceOpenConfiguration.configuration;config.activates=NO;__block BOOL complete=NO;
        [[NSWorkspace sharedWorkspace] openApplicationAtURL:url configuration:config completionHandler:^(NSRunningApplication *running,NSError *error){complete=YES;}];
        NSDate *end=[NSDate dateWithTimeIntervalSinceNow:10];while(!complete&&end.timeIntervalSinceNow>0)[NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.05]];
    }
    if(![app isRunning])return success(@{@"status":@"not_running",@"automation_granted":@NO,@"connected_accounts":@0});
    BOOL ask=[operation isEqual:@"authorize"];OSStatus access=permission(ask);
    if(access!=noErr){if([operation isEqual:@"status"]||ask)return success(@{@"status":access==errAEEventWouldRequireUserConsent?@"permission_required":@"permission_denied",@"automation_granted":@NO,@"os_status":@(access)});return failure(@"messagesAutomationPermissionRequired",@"Automation access to Messages is unavailable. Run delivery authorize and grant the macOS permission before sending.");}
    app.timeout=60*10;app.sendMode=kAEWaitReply|kAENeverInteract;
    FailureObserver *observer=[FailureObserver new];app.delegate=observer;
    NSArray *accounts=connected(app,observer);if(!accounts)return failure(@"messagesApplicationUnavailable",@"Could not read the Messages account state");
    if(![operation isEqual:@"send"])return success(@{@"status":accounts.count?@"ready":@"no_connected_account",@"automation_granted":@YES,@"connected_accounts":@(accounts.count)});
    if(!accounts.count)return failure(@"messagesAccountUnavailable",@"Messages has no connected account; no message was sent");
    NSDictionary *target=request[@"target"];NSString *body=request[@"text"];if((id)body==NSNull.null)body=nil;NSArray *files=request[@"files"];
    if(![target isKindOfClass:NSDictionary.class]||![files isKindOfClass:NSArray.class]||files.count>10||(body&&(![body isKindOfClass:NSString.class]||!body.length))||(!body&&files.count==0))return failure(@"invalidArguments",@"Invalid native send payload");
    NSArray *addresses=target[@"recipients"];NSSet *expected=members(addresses);if(![addresses isKindOfClass:NSArray.class]||addresses.count<1||addresses.count>100||!expected)return failure(@"invalidArguments",@"No resolved native recipients");
    for(NSDictionary *file in files){if(![file isKindOfClass:NSDictionary.class])return failure(@"invalidArguments",@"Invalid attachment metadata");NSDictionary *error=checkFile(file);if(error)return error;}
    CUMChat *chat=nil;CUMAccount *account=nil;id destination=nil;
    if([target[@"kind"] isEqual:@"existing"]){
        NSString *guid=target[@"chat_guid"];if(![guid isKindOfClass:NSString.class])return failure(@"invalidArguments",@"Invalid chat identifier");
        chat=[[app chats] objectWithID:guid];NSString *actual=[chat id];account=[chat account];
        if(observer.error||![actual isEqual:guid])return failure(@"messagesSendPlanChanged",@"The approved Messages chat is no longer available");
        NSMutableArray *actualMembers=[NSMutableArray array];for(CUMParticipant *person in [[chat participants] get]){NSString *address=[person handle];if(!address)return failure(@"messagesSendPlanChanged",@"Chat membership could not be verified");[actualMembers addObject:address];}
        if(observer.error||![expected isEqual:members(actualMembers)]||![service(account.serviceType) isEqual:target[@"service"]]||!account.enabled||account.connectionStatus!='conn')return failure(@"messagesSendPlanChanged",@"The chat's membership, service or account changed; no message was sent");
        destination=chat;
    }else if([target[@"kind"] isEqual:@"new"]){
        // Prefer iMessage. A phone-only destination can use a connected RCS or SMS relay account.
        for(CUMAccount *candidate in accounts)if(candidate.serviceType=='sims'){account=candidate;break;}
        BOOL phoneOnly=YES;for(NSString *address in addresses)if([address containsString:@"@"])phoneOnly=NO;
        if(!account&&phoneOnly){for(NSNumber *kind in @[@((unsigned int)'srcs'),@((unsigned int)'ssms')]){for(CUMAccount *candidate in accounts)if(candidate.serviceType==kind.unsignedIntValue){account=candidate;break;}if(account)break;}}
        if(!account)return failure(@"messagesAccountUnavailable",@"No connected account can address the approved new conversation");
        NSMutableArray *peers=[NSMutableArray array];for(NSString *address in addresses){
            CUMParticipant *peer=[[account participants] objectWithID:[NSString stringWithFormat:@"%@:%@",account.id,address]];NSString *handle=[peer handle];
            if(observer.error||![normalizeAddress(handle?:@"") isEqual:normalizeAddress(address)]){observer.error=nil;peer=[[account participants] objectWithName:address];handle=[peer handle];}
            if(observer.error||![normalizeAddress(handle?:@"") isEqual:normalizeAddress(address)])return failure(@"messagesAmbiguousDestination",@"Messages could not resolve an approved recipient exactly");[peers addObject:peer];
        }
        if(peers.count==1)destination=peers[0];else{
            if(!alive(request))return failure(@"messagesSendCanceled",@"The send was canceled before creating a conversation");
            NSMutableArray *participantIDs=[NSMutableArray array];for(CUMParticipant *peer in peers){NSString *identifier=[peer id];if(!identifier.length||observer.error)return failure(@"messagesSendPlanUnavailable",@"Messages could not identify an approved group member");[participantIDs addObject:identifier];}
            NSError *creationError=nil;NSString *createdID=createGroup(account.id,participantIDs,&creationError);if(creationError)observer.error=creationError;
            if(createdID.length)chat=[[app chats] objectWithID:createdID];
            if(observer.error||!chat.id.length){
#ifdef CUA_FIXTURE_SELF
                fprintf(stderr,"Fixture group error: %s\n",observer.error.description.UTF8String);
#endif
                return failure(@"messagesSendPlanUnavailable",@"Messages could not create the approved group; no message was sent");
            }
            NSMutableArray *created=[NSMutableArray array];for(CUMParticipant *peer in [[chat participants] get]){if(!peer.handle)return failure(@"messagesSendPlanChanged",@"New group membership could not be verified");[created addObject:peer.handle];}
            if(observer.error||![expected isEqual:members(created)])return failure(@"messagesSendPlanChanged",@"Created group membership did not match the review; no message was sent");destination=chat;
        }
    }else return failure(@"invalidArguments",@"Invalid native destination kind");
    if(observer.error)return failure(@"messagesSendPlanUnavailable",@"The native send destination could not be verified");
    if(!alive(request))return failure(@"messagesSendCanceled",@"The send was canceled before dispatch");
    if(body){*dispatched=YES;[app send:body to:destination];if(observer.error)return failure(@"messagesSendOutcomeUncertain",@"Messages did not confirm the text send; do not retry automatically");}
    for(NSDictionary *file in files){
        if(!alive(request))return failure(*dispatched?@"messagesSendOutcomeUncertain":@"messagesSendCanceled",@"The request ended before all attachments were acknowledged");
        NSDictionary *error=checkFile(file);if(error)return *dispatched?failure(@"messagesSendOutcomeUncertain",@"An attachment changed after part of the send was dispatched"):error;
        *dispatched=YES;[app send:[NSURL fileURLWithPath:file[@"staged"]] to:destination];if(observer.error)return failure(@"messagesSendOutcomeUncertain",@"Messages did not confirm an attachment send; do not retry automatically");
    }
    NSString *guid=chat?[chat id]:nil;if(observer.error)guid=nil;
    return success(@{@"status":@"accepted",@"chat_guid":guid?:NSNull.null,@"acknowledged_by":@"Messages AppleEvents",@"attachment_count":@(files.count)});
}
int main(int argc,const char *argv[]){@autoreleasepool{
    BOOL dispatched=NO;
    @try{
        NSMutableData *bytes=[NSMutableData data];NSFileHandle *input=NSFileHandle.fileHandleWithStandardInput;
        while(bytes.length<=1024*1024){NSData *part=[input readDataOfLength:MIN(65536,1024*1024+1-bytes.length)];if(!part.length)break;[bytes appendData:part];}
        id request=[NSJSONSerialization JSONObjectWithData:bytes options:0 error:nil];if(bytes.length>1024*1024||![request isKindOfClass:NSDictionary.class]){emit(failure(@"invalidArguments",@"Invalid Messages helper request"));return 0;}
        NSString *nonce=request[@"nonce"];if(nonce)emit(@{@"event":@"started",@"nonce":nonce,@"pid":@(getpid())});
        BOOL authorization=[request[@"operation"] isEqual:@"authorize"];NSDate *deadline=[NSDate dateWithTimeIntervalSinceNow:authorization?130:30];
        dispatch_source_t watchdog=dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER,0,0,dispatch_get_global_queue(QOS_CLASS_UTILITY,0));
        dispatch_source_set_event_handler(watchdog,^{struct rusage usage;getrusage(RUSAGE_SELF,&usage);if(deadline.timeIntervalSinceNow<=0||usage.ru_maxrss>512*1024*1024||!alive(request))_exit(2);});dispatch_source_set_timer(watchdog,dispatch_time(DISPATCH_TIME_NOW,50*NSEC_PER_MSEC),50*NSEC_PER_MSEC,5*NSEC_PER_MSEC);dispatch_resume(watchdog);
        emit(run(request,&dispatched));dispatch_source_cancel(watchdog);return 0;
    }@catch(NSException *exception){emit(failure(dispatched?@"messagesSendOutcomeUncertain":@"messagesNativeHelperUnavailable",dispatched?@"The native helper stopped after dispatch; do not retry automatically":@"The native Messages operation could not be completed"));return 0;}
}}
