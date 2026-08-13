# iRacing Benchmark Program

## Usage

### Initial setup

1. Go to the `bin` folder and right click the `ir-benchmark-rs-gui.exe` file and select `Properties`. In the `Compatibility` tab, check the `Run this program as an administrator` option. Click `Apply` and then `OK`. This is necessary for the benchmark program to be able to log the benchmark results.
2. Copy the replay folders/files within this folder to your iRacing folder (`.../My Documents/iRacing/replay`).
3. Go to the `bin/input` folder and rename the folder `REPLACE FOLDER NAME AND CONTENTS` to a useful name which describes its main difference to the other input folders. Examples: `Default`, `Reflex Off - No FPS Limit`, `Reflex Off - FPS Limit 144`.
4. Again in the `bin/input` folder, fill in the `benchmark_order.ir_bench.json5` file with the new folder name and any other input folders you have created. The order in which the benchmarks will be run is defined by the id of the benchmarks. Examples of the `benchmark_order.ir_bench.json5` file are shown below. Save the file after editing.
5. Copy the `rendererDX11Monitor.ini` file from the `.../My Documents/iRacing` folder to the `bin/input` subfolders. This file is your baseline graphics settings for the benchmark. In order to test different graphics settings, you can edit the `rendererDX11Monitor.ini` file in each of the input subfolders. The benchmark program will use the `rendererDX11Monitor.ini` file from each input subfolder when running the benchmarks. You can also test different `app.ini` settings by copying the `app.ini` file from the `.../My Documents/iRacing` folder to the `bin/input` subfolders and editing it there.
6. Go to the `bin` folder and run the `ir-benchmark-rs-gui.exe` file.
7. Copy your iRacing folder path (`.../My Documents/iRacing`) and paste it in the `iracing_folder` field. After that, click the `Save Config` button.

#### benchmark_order.ir_bench.json5

Initial contents:

```json5
{
    benchmarks: [
        {
            id: 1,
            path: "./REPLACE FOLDER NAME AND CONTENTS",
        },
    ],
}
```

Example contents after editing:

```json5
{
    benchmarks: [
        {
            id: 1,
            path: "./Default",
        },
        {
            id: 2,
            path: "./Reflex Off - No FPS Limit",
        },
        {
            id: 3,
            path: "./Reflex Off - FPS Limit 144",
        },
    ],
}
```

### Running the program

1. Go to the `bin` folder and run the `ir-benchmark-rs-gui.exe` file.
2. If everything is set up correctly, you can click the `Start` button. This will start the benchmark program.
   - The program will automatically run the benchmark visualization program after the benchmark is finished. The results will be saved in the `bin/results/<BENCHMARK FOLDER NAME>` folder. It's not necessary to fill in the `Bench Visualization Input Folder` field.
   - If you want to run the benchmark visualization program directly (to see the results of a previous benchmark), you need to fill in the `Bench Visualization Input Folder` field with the path to the `bin/results/<BENCHMARK FOLDER NAME>` folder. You can find the benchmark folder name(s) in the `bin/results` folder. After that, click the `Open Visualization` button.
3. The program will wait for a replay file to be loaded in iRacing. After the replay file is loaded, the program will automatically start the benchmark. The benchmark will run until the replay file is finished. After that, the program will automatically close iRacing and start the next benchmark run (if any). If you want to stop the benchmark program, you can click the `Stop` button.
4. If there are multiple benchmarks to run, it'll wait after the last run of a benchmark for the user to click the `Quit` button in iRacing. After that, you must load into the replay file again.
5. When the last benchmark run of the last benchmark is finished, you should again disconnect from iRacing and then the program will generate the benchmark visualization results and open those in you web browser.

### Custom replay files

If you want to test a different track, car, or weather conditions, you can create your own replay file(s).
There are a few things that the benchmark program expects from the replay files:

- The replay file must contain a single session, preferably a race session.
- The replay file must be short enough to be able to fully run the benchmark program in a reasonable amount of time, multiple times.
