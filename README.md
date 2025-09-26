# Headless LED Wall

[![Firmware](https://github.com/florianL21/headless-led-wall/actions/workflows/embedded.yml/badge.svg)](https://github.com/florianL21/headless-led-wall/actions/workflows/embedded.yml)
[![Server](https://github.com/florianL21/headless-led-wall/actions/workflows/server.yml/badge.svg)](https://github.com/florianL21/headless-led-wall/actions/workflows/server.yml)

<p align="center">
  <img src="https://github.com/florianL21/headless-led-wall/blob/main/demo.gif?raw=true" alt="Example dashboard"/>
</p>

# Overview
This project provides a ready-made, generic solution for building an LED wall to display some dashboard-like information.

It comes with a finished example for displaying departure times for the public transport in Vienna and the weather forecast for the day.

This repo consists of 2 parts:

* The embedded firmware
* The server implementation

The embedded firmware is quite flexible and should accommodate for a wide range of information which can be displayed.
The embedded device simply connects to WIFI on boot and opens a REST API over which anyone in the network can send an HTTP POST request to configure what the display currently renders.

The actual content displayed is completely independent of the embedded code, and can be as simple as a statically configured json file.

This repo provides a ready made implementation of the server side as well, which besides providing a set of utilities to manage the display also provides the implementation of a long running server to display the exact dashboard seen in the example.

# Who this project is for

* People who want to replicate exactly the same dashboard as shown in the example
* People who may know other programming languages than rust and want to write their own dashboards via the json interface of the server implementation
* People who are learning rust and want a ready made project which covers both the embedded (no_std) side and application development (std) to hack around in and see some quick results on a physical device
* Anyone who knows rust and wants to use this as a starting point to create their own dashboards

# Step by step guide

1. Build the hardware like described in the Hardware section below
2. Modify/reference the GPIO pins in [main.rs](embedded/src/bin/main.rs) to match with the real hardware. Look for the instantiation of the `Hub75Pins8` struct.
3. Make sure you have rust installed (https://rustup.rs/)
4. In a terminal run `cargo install --locked espup espflash`
5. Run `espup install`
6. Make a copy of [config.toml.template](embedded/config.toml.template) in the same directory and name it `config.toml`. Open the file and follow the comment to make the necessary changes. At the very minimum you have to fill out the WIFI details.
7. Plug your ESP into your PC, navigate to the `embedded` directory and run `cargo run --release`
8. Make a copy of [config.toml.template](server/config.toml.template) in the same directory and name it `config.toml`. Open it and follow the comments to make adjustments. At the very least you will have to configure the IP address of the display. The ESP will have printed its IP address on the console as part of the previous step.
9. Navigate to the `server` directory and run `cargo run -- config.toml server`
10. You should now have a dashboard like shown in the example picture

If you run into any issues or want to get a deeper understanding of how things are working have a look at the following sections.

# The hardware

This projects uses an esp32-s3 together with commonly available LED matrix panels with HUB75 style connectors.
These panels are chained together electrically in one long string and arranged in a grid to make up a single big display.
For more details about which panels should work please consult the documentation of the underlying library: [hub75-framebuffer](https://crates.io/crates/hub75-framebuffer)

The ESP is connected to the panel via the circuit described in the [hub75-framebuffer's documentation here](https://github.com/liebman/hub75-framebuffer?tab=readme-ov-file#the-latch-circuit)

When looking at the front, the panels are connected from the top right, running to the left, then down to the next row.
The second row of panels is thus mounted upside down, the cables running from left to right.
This pattern is repeated for every two rows in the arrangement.

# The software

## Embedded

On the embedded side there is a static firmware that is flashed as-is (after configuring the LED panel parameters and WIFI) to the esp32.

When the ESP starts it will attempt to connect to WIFI indefinitely until it has connected and received an IP address over DHCP.
It will then start displaying the current configuration.

It will also open a REST API with which one can interact. The current endpoints are:

 * `/api/state` -> POST to tun the display on/off. For example `/api/state?on=false` will turn the display off
 * `/api/config` -> POST to change what is displayed on the LED panel.
   This request has no parameters and its body needs to be a correctly formatted [postcard message](https://postcard.jamesmunns.com/).
   This message additionally needs to conform to the [schema.json](server/schema.json)
 * `/api/settings` -> POST to change display settings. Currently only brightness is supported. For example `/api/settings?brightness=50` will set the display to 50% brightness
 * `/api/storage/format` -> POST to format the whole sprite flash "file system"
 * `/api/storage/upload` -> POST to upload a single sprite. The body needs to be a correctly formatted [postcard message](https://postcard.jamesmunns.com/).
   For example `/api/storage/upload?key=test` will upload the sprite in the request body to the internal flash of the ESP under then mae "test".
 * `/api/storage/exists` -> POST to check if a sprite with a given name exists.
   For example `/api/storage/exists?key=test` will check if a sprite with the name test exists.
   Currently the response is only a human readable string.
 * `/api/storage/delete` -> POST to delete a given sprite from internal flash. For example `/api/storage/delete?key=test` will delete the sprite called "test".

### Flashing

To compile and flash the firmware to the ESP simply run:

```bash
cargo run --release
```

### Troubleshooting

If the ESP is crashing/hanging or not starting up properly start by having a look at the following configuration files and read the comments in them:

* [embedded/.cargo/config.toml](embedded/.cargo/config.toml)

#### ESP does not start

This can have multiple reasons. Most likely there is a configuration issue with the chips peripherals.

A panic which looks like this:

```
I (364) boot: Loaded app from partition at offset 0x10000
I (364) boot: Disabling RNG early entropy source...
INFO - chip id = ffffff
INFO - size is 0
INFO - PSRAM core_clock SpiTimingConfigCoreClock240m, flash_div = 2, psram_div = 2

====================== PANIC ======================
...
```

Is usually an indicator for a miss-configured PSRAM interface. In this case go to [config.toml](embedded/.cargo/config.toml) and check the comment above `ESP_HAL_CONFIG_PSRAM_MODE`



## Server

This repo also contains an application for talking to the headless display for doing some mainainance tasks,
like formatting the internal storage, pushing a display update from a json file, uploading image sprites to the ESPs internal flash, etc.

This application also contains a reference implementation for a ready made dashboard.
This dashboard will show data from the Winer Linien API for public transport in Vienna, and some weather data from the [Meteorologisk institutt](https://www.met.no/) via their API.
Exact lines to track and location for th weather service can be set via the [the server config.toml](server/config.toml).
If your only goal is to replicate the exact dashboard shown in this repos example, it is as easy as configuring some things in [the server config.toml file](server/config.toml) (just follow the comments in the file) and then run the server like so:

```bash
cd server
cargo run -- config.toml server
```

### Utilities in the server application

The server comes with a CLI. This CLI contains more than just the server implementation.
It comes with many utilities that are useful for managing and even running a custom dashboard.

In general all utilities can easily be executed by going to the `server` folder and running `cargo run`. To see all the utilities available run the following command:

```bash
cargo run -- config.toml --help
```

The most notable of these commands are the following:

* `server` -> This will start the long running server process to continuously push updates to the display
* `push-config` -> This pushes the contents of a given JSON file to the display. This is a great way to display static information, or build dashboards using any other programming language than rust.
* `bulk-upload` -> Uploads a set of sprites to the display so it can display them. This takes a configuration file which lists all avaliable sprites. An example of such a file can be found under [resources/sprites/sprites.toml](resources/sprites/sprites.toml)


### Modifying the server

If you want to customize the dashboard in any way you should check the `build_display` function in [display.rs](server/src/display.rs). It should be fairly simple to understand and modify for your needs.

If you need to change something about the data fetching from Wiener Linien or MET you should look at the [wl.rs](server/src/wl.rs) and [weather.rs](server/src/cli.rs) files respectively.

If you need to add an additional datasource have a look at the [server.rs](server/src/server.rs) file.
The `fetch_transport_data` and `fetch_weather_data` tasks should give a good understanding how to periodically fetch data from some source and then send it to the `push_display_update` task.
The `push_display_update` receives this data and then sends it to the display after transforming it to a dashboard by calling the `build_display` function from before.

To summarize, for adding a new datasource you will need to:
#. Add an additional fetch task
#. Extend the `DataUpdate` enum with a new variant for your new data and update message
#. Receive this new update message in the `push_display_update` and pass it onto the `build_display` function
#. Render your data to the display in the `build_display` function.


### Docker

For deploying the server in a more permanent way, a docker container is an easy choice.
The current Dockerfile embeds the configuration into the container for easy deployment, but with this it means that the docker image will contain some sensitive private information. It is intended to be pushed to a locally running docker registry.

So: !!!DO NOT PUSH IT TO THE DOCKER HUB!!!

For example:

```bash
make build DOCKER_TAG=docker.l.at/pub-transp-disp:test
```

## The sprites

This projects delivers a set of sprites loosely based but mostly re-drawn in a pixel style on random pictures from google images. I am by no means a capable pixel artist, so some of them may be very rough.

As for the weather icons, these are mostly incomplete. [A complete list of icons the MET API](https://github.com/metno/weathericons/tree/main/weather) may respond with is attached below. Checked off icons are implemented, all unchecked ones are missing:

 - [ ] clearsky _day/night
   - [x] _day
   - [x] _night
   - [ ] _polartwilight
 - [x] cloudy
 - [ ] fair
   - [x] _day
   - [x] _night
   - [ ] _polartwilight
 - [ ] fog
 - [x] heavyrain
 - [x] heavyrainandthunder
 - [ ] heavyrainshowers
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] heavyrainshowersandthunder
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] heavysleet
 - [ ] heavysleetandthunder
 - [ ] heavysleetshowers
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] heavysleetshowersandthunder
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] heavysnow
 - [ ] heavysnowandthunder
 - [ ] heavysnowshowers
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] heavysnowshowersandthunder
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [x] lightrain
 - [ ] lightrainandthunder
 - [ ] lightrainshowers
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] lightrainshowersandthunder
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] lightsleet
 - [ ] lightsleetandthunder
 - [ ] lightsleetshowers
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] lightsnow
 - [ ] lightsnowandthunder
 - [ ] lightsnowshowers
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] lightssleetshowersandthunder
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] lightssnowshowersandthunder
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] partlycloudy
   - [x] _day
   - [x] _night
   - [ ] _polartwilight
 - [x] rain
 - [x] rainandthunder
 - [ ] rainshowers
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] rainshowersandthunder
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] sleet
 - [ ] sleetandthunder
 - [ ] sleetshowers
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] sleetshowersandthunder
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] snow
 - [ ] snowandthunder
 - [ ] snowshowers
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
 - [ ] snowshowersandthunder
   - [ ] _day
   - [ ] _night
   - [ ] _polartwilight
