import Foundation
import Contacts
import AppKit
import Darwin

// No normal query requests permission or launches another app. Authorization is an explicit command.
struct BridgeError: Error { let code: String; let message: String }
func fail(_ code: String, _ message: String) -> BridgeError { BridgeError(code: code, message: message) }
func authorization() -> (String, Bool) {
    let status = CNContactStore.authorizationStatus(for: .contacts)
    if status == .authorized { return ("authorized", false) }
    if status.rawValue == 4 { return ("limited", true) }
    if status == .notDetermined { return ("not_determined", false) }
    return (status == .restricted ? "restricted" : "denied", false)
}
let keys: [CNKeyDescriptor] = [CNContactIdentifierKey as CNKeyDescriptor,
    CNContactGivenNameKey as CNKeyDescriptor, CNContactFamilyNameKey as CNKeyDescriptor,
    CNContactNicknameKey as CNKeyDescriptor, CNContactOrganizationNameKey as CNKeyDescriptor,
    CNContactPhoneNumbersKey as CNKeyDescriptor, CNContactEmailAddressesKey as CNKeyDescriptor,
    CNContactFormatter.descriptorForRequiredKeys(for: .fullName)]
func person(_ contact: CNContact) -> [String: Any] {
    let full = CNContactFormatter.string(from: contact, style: .fullName) ?? ""
    let name = !full.isEmpty ? full : (!contact.organizationName.isEmpty ? contact.organizationName : contact.nickname)
    let handles = contact.phoneNumbers.map { $0.value.stringValue } + contact.emailAddresses.map { String($0.value) }
    return ["identifier": contact.identifier, "name": name, "handles": Array(Set(handles)).sorted()]
}
func run(_ request: [String: Any]) throws -> [String: Any] {
    guard let operation = request["operation"] as? String else { throw fail("invalidArguments", "Missing native Contacts operation") }
    if operation == "status" { let a = authorization(); return ["status": a.0, "limited": a.1] }
    if operation == "authorize" {
        if authorization().0 == "not_determined" {
            let app = NSApplication.shared
            app.setActivationPolicy(.accessory)
            app.finishLaunching()
            app.activate(ignoringOtherApps: true)
            let lock = NSLock(); var finished = false; var authorizationError: NSError?
            let store = CNContactStore()
            defer { withExtendedLifetime(store) {} }
            store.requestAccess(for: .contacts) { _, error in lock.lock(); authorizationError = error as NSError?; finished = true; lock.unlock() }
            let deadline = Date().addingTimeInterval(120)
            while true {
                lock.lock(); let done = finished; lock.unlock()
                if done { break }
                guard Date() < deadline else { throw fail("messagesContactsPermissionPending", "Contacts authorization has not completed") }
                RunLoop.current.run(until: Date().addingTimeInterval(0.05))
            }
            if let error = authorizationError { throw fail("messagesContactsPermissionDenied", "Contacts authorization failed (\(error.domain) \(error.code)): \(error.localizedDescription)") }
        }
        let a = authorization(); return ["status": a.0, "limited": a.1]
    }
    guard operation == "name" || operation == "address" else { throw fail("invalidArguments", "Unknown native Contacts operation") }
    guard let query = request["query"] as? String, !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, query.utf8.count <= 4096 else { throw fail("invalidArguments", "Contacts query must be nonempty and bounded") }
    let status = authorization()
    guard status.0 == "authorized" || status.0 == "limited" else {
        throw fail(status.0 == "not_determined" ? "messagesContactsPermissionRequired" : "messagesContactsPermissionDenied", "Contacts access has not been granted to the native helper. Use contacts authorize or macOS Privacy settings.")
    }
    let predicate: NSPredicate
    if operation == "name" { predicate = CNContact.predicateForContacts(matchingName: query) }
    else if query.contains("@") { predicate = CNContact.predicateForContacts(matchingEmailAddress: query) }
    else { predicate = CNContact.predicateForContacts(matching: CNPhoneNumber(stringValue: query)) }
    let contacts = try CNContactStore().unifiedContacts(matching: predicate, keysToFetch: keys)
    guard contacts.count <= 200 else { throw fail("messagesContactsTooManyMatches", "Use a more specific contact name") }
    return ["people": contacts.map(person), "limited": status.1]
}
func emit(_ value: [String: Any]) {
    let data = (try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])) ?? Data()
    if data.count > 2 * 1024 * 1024 {
        emit(["ok": false, "code": "messagesContactsTooManyMatches", "message": "Contacts response exceeded its size limit"])
    } else { FileHandle.standardOutput.write(data); FileHandle.standardOutput.write(Data([10])) }
}
if CommandLine.arguments.dropFirst().contains("--authorize") {
    do { emit(["ok": true, "data": try run(["operation":"authorize"])]) }
    catch let error as BridgeError { emit(["ok":false,"code":error.code,"message":error.message]) }
    catch { emit(["ok":false,"code":"messagesContactsUnavailable","message":"Contacts authorization failed"]) }
} else if CommandLine.arguments.dropFirst().contains("--help") {
    print("Copilot native Contacts helper: private JSON stdin/stdout protocol")
} else {
    do {
        var data = Data()
        while data.count <= 1024 * 1024 {
            let part = try FileHandle.standardInput.read(upToCount: min(65536, 1024 * 1024 + 1 - data.count)) ?? Data()
            if part.isEmpty { break }; data.append(part)
        }
        guard data.count <= 1024 * 1024, let request = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw fail("invalidArguments", "Invalid native Contacts request") }
        let watchdog = DispatchSource.makeTimerSource(queue: .global(qos: .utility))
        let deadline = Date().addingTimeInterval(request["operation"] as? String == "authorize" ? 130 : 8)
        let lifetime = request["lifetime"] as? String
        watchdog.setEventHandler {
            var usage = rusage(); _ = getrusage(RUSAGE_SELF, &usage)
            if Date() >= deadline || usage.ru_maxrss > 512 * 1024 * 1024 || (lifetime != nil && !FileManager.default.fileExists(atPath: lifetime!)) { _exit(2) }
        }
        watchdog.schedule(deadline: .now() + 0.05, repeating: 0.05); watchdog.resume()
        defer { watchdog.cancel() }
        if let nonce = request["nonce"] as? String { emit(["event":"started","nonce":nonce,"pid":ProcessInfo.processInfo.processIdentifier]) }
        emit(["ok": true, "data": try run(request)])
    } catch let error as BridgeError { emit(["ok": false, "code": error.code, "message": error.message]) }
    catch { emit(["ok": false, "code": "messagesContactsUnavailable", "message": "The native Contacts query could not be completed"]) }
}
