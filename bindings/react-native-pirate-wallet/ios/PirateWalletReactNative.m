#import <Foundation/Foundation.h>
#import <React/RCTBridgeModule.h>
@import PirateWalletNative;

@interface PirateWalletReactNative : NSObject <RCTBridgeModule>
@end

@implementation PirateWalletReactNative

RCT_EXPORT_MODULE();

+ (BOOL)requiresMainQueueSetup
{
  return NO;
}

RCT_REMAP_METHOD(invoke,
                 invoke:(NSString *)requestJson
                 pretty:(BOOL)pretty
                 resolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject)
{
  const char *requestCString = [requestJson UTF8String];
  if (requestCString == NULL) {
    reject(@"PIRATE_WALLET_INVOKE_ERROR", @"Request string was not valid UTF-8.", nil);
    return;
  }

  char *responsePtr = pirate_wallet_service_invoke_json(requestCString, pretty);
  if (responsePtr == NULL) {
    reject(@"PIRATE_WALLET_INVOKE_ERROR", @"Wallet service returned a null response.", nil);
    return;
  }

  NSString *response = [NSString stringWithUTF8String:responsePtr];
  pirate_wallet_service_free_string(responsePtr);

  if (response == nil) {
    reject(@"PIRATE_WALLET_INVOKE_ERROR", @"Wallet service returned invalid UTF-8.", nil);
    return;
  }

  resolve(response);
}

RCT_REMAP_METHOD(configureAccountStorage,
                 configureAccountStorage:(NSString *)accountId
                 passphrase:(NSString *)passphrase
                 storagePath:(NSString *)storagePath
                 resolver:(RCTPromiseResolveBlock)resolve
                 rejecter:(RCTPromiseRejectBlock)reject)
{
  if ((id)storagePath == [NSNull null]) {
    storagePath = nil;
  }

  if (accountId == nil || accountId.length == 0) {
    reject(@"PIRATE_WALLET_CONFIGURE_STORAGE_ERROR", @"accountId must not be empty", nil);
    return;
  }
  if (passphrase == nil || passphrase.length == 0) {
    reject(@"PIRATE_WALLET_CONFIGURE_STORAGE_ERROR", @"passphrase must not be empty", nil);
    return;
  }

  NSError *error = nil;
  NSString *baseDir = [self storagePathForAccountId:accountId storagePath:storagePath error:&error];
  if (baseDir == nil) {
    reject(@"PIRATE_WALLET_CONFIGURE_STORAGE_ERROR", error.localizedDescription, error);
    return;
  }

  NSDictionary *request = @{
    @"method": @"configure_wallet_storage",
    @"base_dir": baseDir,
    @"passphrase": passphrase
  };
  NSData *requestData = [NSJSONSerialization dataWithJSONObject:request options:0 error:&error];
  if (requestData == nil) {
    reject(@"PIRATE_WALLET_CONFIGURE_STORAGE_ERROR", error.localizedDescription, error);
    return;
  }

  NSString *requestJson = [[NSString alloc] initWithData:requestData encoding:NSUTF8StringEncoding];
  if (requestJson == nil) {
    reject(@"PIRATE_WALLET_CONFIGURE_STORAGE_ERROR", @"Storage configuration request was not valid UTF-8.", nil);
    return;
  }

  const char *requestCString = [requestJson UTF8String];
  char *responsePtr = pirate_wallet_service_invoke_json(requestCString, NO);
  if (responsePtr == NULL) {
    reject(@"PIRATE_WALLET_CONFIGURE_STORAGE_ERROR", @"Wallet service returned a null response.", nil);
    return;
  }

  NSString *response = [NSString stringWithUTF8String:responsePtr];
  pirate_wallet_service_free_string(responsePtr);

  if (response == nil) {
    reject(@"PIRATE_WALLET_CONFIGURE_STORAGE_ERROR", @"Wallet service returned invalid UTF-8.", nil);
    return;
  }

  resolve(response);
}

- (NSString *)storagePathForAccountId:(NSString *)accountId
                          storagePath:(NSString *)storagePath
                                error:(NSError **)error
{
  if (storagePath != nil && storagePath.length > 0) {
    return [self ensureStorageDirectory:storagePath error:error] ? storagePath : nil;
  }

  NSString *sanitized = [self sanitizedAccountId:accountId];
  if (sanitized.length == 0) {
    if (error != nil) {
      *error = [NSError errorWithDomain:@"PirateWalletReactNative"
                                   code:1
                               userInfo:@{NSLocalizedDescriptionKey: @"accountId must not be empty"}];
    }
    return nil;
  }

  NSArray<NSURL *> *urls = [[NSFileManager defaultManager] URLsForDirectory:NSApplicationSupportDirectory
                                                                  inDomains:NSUserDomainMask];
  NSURL *applicationSupport = urls.firstObject;
  if (applicationSupport == nil) {
    if (error != nil) {
      *error = [NSError errorWithDomain:@"PirateWalletReactNative"
                                   code:2
                               userInfo:@{NSLocalizedDescriptionKey: @"Application Support directory is unavailable"}];
    }
    return nil;
  }

  NSURL *base = [[applicationSupport URLByAppendingPathComponent:@"PirateWallet" isDirectory:YES]
    URLByAppendingPathComponent:@"accounts" isDirectory:YES];
  NSString *path = [[base URLByAppendingPathComponent:sanitized isDirectory:YES] path];
  return [self ensureStorageDirectory:path error:error] ? path : nil;
}

- (BOOL)ensureStorageDirectory:(NSString *)path error:(NSError **)error
{
  return [[NSFileManager defaultManager] createDirectoryAtPath:path
                                   withIntermediateDirectories:YES
                                                    attributes:nil
                                                         error:error];
}

- (NSString *)sanitizedAccountId:(NSString *)accountId
{
  NSString *trimmed = [accountId stringByTrimmingCharactersInSet:[NSCharacterSet whitespaceAndNewlineCharacterSet]];
  if (trimmed.length == 0) {
    return @"";
  }

  NSMutableString *result = [NSMutableString stringWithCapacity:trimmed.length];
  NSCharacterSet *allowed = [NSCharacterSet characterSetWithCharactersInString:@"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-"];
  for (NSUInteger index = 0; index < trimmed.length; index++) {
    unichar character = [trimmed characterAtIndex:index];
    if ([allowed characterIsMember:character]) {
      [result appendFormat:@"%C", character];
    } else {
      [result appendString:@"_"];
    }
  }
  return result;
}

@end
