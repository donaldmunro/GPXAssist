# GPXAssist

## Releases

Releases: TBD

Note: I do not currently own an Apple device or Apple developer account, so there are no MacOS builds currently available (see the Usage section below for building it). 
(Maybe I'll get an Apple Mini if the M4 prices drop due to the M5 coming out).

Currently GPXAssist is only available for desktop, possibly a mobile version will be made in the future (or a WASM version that can run as a local server).

## Description

GPXAssist is an addon application for  [TrainingPeaks Virtual's](https://www.trainingpeaks.com/virtual/]) new [GPXplore](https://www.trainingpeaks.com/blog/gpxplorer-trainingpeaks-virtual/) functionality.  

It provides:

* In-ride map display with wind direction and speed arrows

<img src="/img/50kmh-wind.png" alt="50Km/h wind" width="960" height="540">

* Google Street View (requires Google API key - free tier has 10000 views per month),

<img src="/img/streetview.png" alt="Street with cars" width="960" height="540">

* and customizable gradient profile

<img src="/img/gradient.png" alt="Gradient" width="960" height="540">

## Usage
The application is distributed using single executable file which can download from the releases page (see links above), or built from source by cloning the repository or downloading the source code zip file, and then run cargo build --release in the source directory (after [installing Rust](https://rust-lang.org/tools/install/)).

It uses the broadcast capability built in to TrainingPeaks Virtual, therefore to use it you need to:

1. Select settings from the main menu (gear icon top right)

<img src="/assets/menu-1.png" alt="Main Menu" width="960">

2. Select "Broadcast Settings" from the settings menu:

<img src="/assets/menu-2.png" alt="Broadcast setting" width="100">

3. Tick the "Save to Local File" option and set a reasonable update interval:

<img src="/assets/menu-3.png" alt="Broadcast file" width="960">

3. Click open <img src="/img/open.png" alt="Broadcast file" width="48">  in GPXAssist and select
the .gpx file of the ride you want to do. This is necessary as currently the data in the broadcast file does not include position (latitude/longitude), therefore GPXAssist needs to read the .gpx file to build a mapping between distance along the route and position. In the future this may not be necessary if TrainingPeaks Virtual includes position in the broadcast data (although the gradient display will still require a mapping of distance to elevation, whether it be in a .gpx file or some other file such as a recording made of distance and altitude while riding on TPV). 

4. Start a ride in TrainingPeaks Virtual, and GPXAssist should automatically detect the broadcast file and start updating the currently selected mode (map, street view or gradient). You will be warned to change the broadcast settings if the broadcast file cannot be found or is older than 1 minute.
 
    The "Delta" option at the top allows you to specify when the display is updated, e.g. if set to 100 then it is updated every 100m (sorry no imperial units yet). 
    The current display is selected by clicking on one of "Map", "Street View" or "Gradient" labels.

5. There is also a test/simulate mode that allows you to load a .gpx file and simulate a ride along it at a specified speed without needing to connect to TrainingPeaks Virtual. To use this mode, open a .gpx file, set the speed in km/h (can be changed during the simulation) and  click the "Start Simulate" <img src="/img/sim.png" alt="Broadcast file" width="40">  button. Click the button (which displays as selected or darker gray while running) again to stop the simulation.