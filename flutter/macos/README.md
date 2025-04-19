# RustDesk on macOS

## Sparkle

https://sparkle-project.org/documentation/customization/

1. `SUAutomaticallyUpdate` is supported. But " In Sparkle 2, updates will be downloaded but not installed automatically if authorization is required.".
2. For `SUScheduledCheckInterval`, "Note: this has a minimum bound of 1 hour in order to keep you from accidentally overloading your servers.".
3. Don't use Debug build for tests. `/Application/RustDesk.app` always show the same version as `/Users/rustdesk/workspace/rust/rustdesk/flutter/build/macos/Build/Products/Debug/RustDesk.app`. Eg, if I install a version `1.3.5`, but then I rebuild a new Version `1.3.1`. `/Application/RustDesk.app/Contents/MacOS/RustDesk --version` will print `1.3.1`, event I have deleted the directory `/Users/rustdesk/workspace/rust/rustdesk/flutter/build`.
