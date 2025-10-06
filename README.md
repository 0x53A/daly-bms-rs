This app currently only runs in the web. There are fragments for native/mobile in this repository from the template used, but those platforms aren't implemented currently, and likely won't be in the near future.

------

## What is it?

This is a web app that allows to control Daly BMS (Battery Management System) over WebBluetooth. It runs on Chrome and Chrome based browsers, including on Android, but NOT Firefox, nor on iOS.

The current version was vibe-coded in an afternoon, so don't expect too much. It can read the voltages:

<img width="826" height="892" alt="image" src="https://github.com/user-attachments/assets/42e9d369-b3e4-4d3f-b403-9dfb59b2d134" />

----

## Web

```
# one time
cargo install --locked trunk

# debug (http://127.0.0.1:8080)
trunk serve 

# release (/dist folder)
trunk build --release
```

## Windows

n/a

## Android

n/a

-----
