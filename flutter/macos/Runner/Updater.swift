// https://github.com/ZzzM/CalendarX/blob/a0d26cf917aecda954a7b6174d2a7648c1c5e43a/CalendarX/Utility/Updater.swift

import Sparkle

class Updater: NSObject {

    private lazy var updaterController = SPUStandardUpdaterController(
        startingUpdater: false,
        updaterDelegate: self,
        userDriverDelegate: nil
    )

    var automaticallyChecksForUpdates: Bool {
        get {
            updaterController.updater.automaticallyChecksForUpdates
        }
        set {
            if (updaterController.updater.automaticallyChecksForUpdates != newValue) {
                updaterController.updater.automaticallyChecksForUpdates = newValue
            }
        }
    }

    override
    init() {
        super.init()
        updaterController.startUpdater()
        automaticallyChecksForUpdates = false
        // updaterController.updater.updateCheckInterval = 3600.0
    }

    func checkForUpdates() {
        updaterController.checkForUpdates(nil)
    }
}

extension Updater: SPUUpdaterDelegate {
    func updaterDidNotFindUpdate(_ updater: SPUUpdater) {
        NSLog("Already the latest version")
    }

    func updater(_ updater: SPUUpdater, didAbortWithError error: Error) {
        NSLog("Failed to check update: \(error.localizedDescription)")
    }

    func updater(_ updater: SPUUpdater, didFindValidUpdate item: SUAppcastItem) {
        let notification = NSUserNotification()
        notification.title = "New Update Available"
        notification.informativeText = "Click to install version \(item.displayVersionString)"
        notification.actionButtonTitle = "Install"
        NSUserNotificationCenter.default.deliver(notification)
    }

    func updaterWillRelaunchApplication(_ updater: SPUUpdater) {
        if (is_installed() && is_installed_daemon() && !is_service_stopped()) {
            self.setShouldRunPostUpdate()
        }
    }

    func checkReinstallService() {
        let args = CommandLine.arguments
        if args.count == 1 {
           if self.shouldRunPostUpdate() {
                DispatchQueue.global().async {
                    do {
                    try self.runShellCommand("""
                        sleep 2
                        /Applications/RustDesk.app/Contents/MacOS/RustDesk --reinstall-service
                        """)
                    } catch
                    {
                        NSLog("[RustDesk] reinstall services failed")
                    }
                }
                self.clearShouldRunPostUpdate()
            }
        }
    }

    func setShouldRunPostUpdate() {
        let defaults = UserDefaults(suiteName: "com.carriez.rustdesk")
        defaults?.set(true, forKey: "ShouldRunPostUpdateScript")
        defaults?.synchronize()

        // `UserDefaults` does not work for me, so I use the file to double confirm.
        try? "1".write(toFile: "/tmp/.rustdesk_update", atomically: true, encoding: .utf8)
    }

    func shouldRunPostUpdate() -> Bool {
        let fileExists = FileManager.default.fileExists(atPath: "/tmp/.rustdesk_update")
        let defaultsFlag = UserDefaults(suiteName: "com.carriez.rustdesk")?.bool(forKey: "ShouldRunPostUpdateScript") ?? false

        return fileExists || defaultsFlag
    }

    func clearShouldRunPostUpdate() {
        let defaults = UserDefaults(suiteName: "com.carriez.rustdesk")
        defaults?.set(false, forKey: "ShouldRunPostUpdateScript")
        defaults?.synchronize()

        try? FileManager.default.removeItem(atPath: "/tmp/.rustdesk_update")
    }

    @discardableResult
    private func runShellCommand(_ command: String) throws -> String {
        let process = Process()
        let pipe = Pipe()
        
        process.executableURL = URL(fileURLWithPath: "/bin/zsh")
        process.arguments = ["-c", command]
        process.standardOutput = pipe
        process.standardError = pipe
        
        try process.run()
        process.waitUntilExit()
        
        let data = pipe.fileHandleForReading.readDataToEndOfFile()
        guard let output = String(data: data, encoding: .utf8) else {
            throw UpdaterError.commandFailed(command: command)
        }
        
        guard process.terminationStatus == 0 else {
            throw UpdaterError.commandFailed(command: "\(command) (exit: \(process.terminationStatus))")
        }
        
        return output
    }
    
    enum UpdaterError: Error {
        case commandFailed(command: String)
    }
}
