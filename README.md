# ustreamer

To run image server, first open `ustreamer` directory and run `cargo run` in terminal. \
Then run the web server by navigating to `server` in a seperate terminal and run `cargo run`.

This is a work in progress, so many errors may occur. \
Currently, the configuration is hardwired for Rockchip devices. 

Tested on:
* RK3588, Armbian 25.5.2 noble


Compared to ustreamer 5.43 with rk3588 HDMI RX Multi-Planar patch

(MAX) mode means there is no frame pacing, so images may slightly stutter between frames

Run on OrangePi 5 Plus using 1920x1080 NV24 stream (Built in HDMI RX Port)
Run with release build of `ustreamer-rs`

Baseline Performance PiKVM ustreamer: ~28 FPS
| **Mode**             | **ustreamer-rs FPS** |   % Performance  |
|----------------------|----------------------|------------------|
| CPU (Single Core)    | ~26 FPS (MAX)        | 93%              |
| CPU Pool (All Cores) | ~40 FPS (MAX)        | 143%             |
| RK-MPP ( HW Accel )  | ~51 FPS (MAX)        | 182%             |


