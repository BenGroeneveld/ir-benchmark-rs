use std::{
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::Ordering,
    thread,
    time::Duration as StdDuration,
};

use anyhow::{Result, anyhow};
use chrono::Local;
use iracing::{
    broadcast::{cam_set_state, replay_search, replay_set_play_speed},
    session::SessionDetails,
    states::CameraState,
    telemetry::{Connection, Sample, Value},
    utils::RpySrchMode,
};
use serde::Deserialize;

use crate::{ConfigData, UiState, get_app_root_dir};

#[derive(Debug, Clone, Copy, Default)]
struct IracingSessionTelemetry
{
    speed_mps:            f32,
    replay_frame_num:     i32,
    cam_car_idx:          i32,
    session_time:         f64,
    replay_frame_num_end: i32,
}

#[derive(Debug, Clone, Deserialize)]
struct BenchmarkOrderEntry
{
    id:   i32,
    path: String,
}

#[derive(Debug, Clone, Deserialize)]
struct BenchmarkOrderFile
{
    benchmarks: Vec<BenchmarkOrderEntry>,
}

fn resolve_relative_path(base: &Path, path: &str) -> PathBuf
{
    let candidate = PathBuf::from(path);
    if candidate.is_absolute()
    {
        candidate
    }
    else
    {
        base.join(candidate)
    }
}

fn load_benchmark_order_file(path: &Path) -> Result<BenchmarkOrderFile>
{
    let content = fs::read_to_string(path)?;
    Ok(json5::from_str(&content)?)
}

fn read_current_bench_id(path: &Path) -> Result<i32>
{
    if !path.is_file()
    {
        return Ok(-1);
    }

    let value = fs::read_to_string(path)?.trim().to_string();
    Ok(value.parse::<i32>().unwrap_or(-1))
}

fn write_current_bench_id(path: &Path, bench_id: i32) -> Result<()>
{
    if let Some(parent) = path.parent()
    {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, bench_id.to_string())?;
    Ok(())
}

fn value_to_f64(v: &Value) -> Option<f64>
{
    match v
    {
        Value::DOUBLE(d) => Some(*d),
        Value::FLOAT(f) => Some(*f as f64),
        Value::INT(i) => Some(*i as f64),
        Value::BOOL(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::CHAR(c) => Some(*c as f64),
        Value::BITS(u) => Some(*u as f64),
        Value::FloatVec(vec) => vec.first().map(|f| *f as f64),
        Value::IntVec(vec) => vec.first().map(|i| *i as f64),
        Value::BoolVec(vec) => vec.first().map(|b| if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn value_to_i32(v: &Value) -> Option<i32>
{
    match v
    {
        Value::INT(i) => Some(*i),
        Value::DOUBLE(d) => Some(*d as i32),
        Value::FLOAT(f) => Some(*f as i32),
        Value::BOOL(b) => Some(if *b { 1 } else { 0 }),
        Value::CHAR(c) => Some(*c as i32),
        Value::BITS(u) => Some(*u as i32),
        Value::IntVec(vec) => vec.first().copied(),
        Value::FloatVec(vec) => vec.first().map(|f| *f as i32),
        Value::BoolVec(vec) => vec.first().map(|b| if *b { 1 } else { 0 }),
        _ => None,
    }
}

fn backup_iracing_ini_files(config: &ConfigData, backup_folder: &Path) -> Result<PathBuf>
{
    if config.iracing_folder.is_empty()
    {
        return Err(anyhow!("iRacing folder not configured"));
    }
    let timestamp = Local::now().format("%Y-%m-%d_%H.%M.%S").to_string();
    let backup_root = Path::new(&backup_folder).join(timestamp);
    fs::create_dir_all(&backup_root)?;

    for entry in fs::read_dir(&config.iracing_folder)?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file()
        {
            continue;
        }
        let file_name = match path.file_name()
        {
            Some(name) => name.to_owned(),
            None => continue,
        };
        let file_name_lower = file_name.to_string_lossy().to_lowercase();
        if file_name_lower.ends_with(".ini") || file_name_lower.contains(".ir_bench.")
        {
            let dest = backup_root.join(file_name);
            if let Some(parent) = dest.parent()
            {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, dest)?;
        }
    }

    Ok(backup_root)
}

fn copy_bench_files_to_iracing_folder(config: &ConfigData, bench_folder: &Path) -> Result<()>
{
    if config.iracing_folder.is_empty()
    {
        return Err(anyhow!("iRacing folder not configured"));
    }
    for entry in fs::read_dir(bench_folder)?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file()
        {
            continue;
        }
        let file_name = match path.file_name()
        {
            Some(name) => name.to_owned(),
            None => continue,
        };
        let file_name_lower = file_name.to_string_lossy().to_lowercase();
        if file_name_lower.ends_with(".ini") || file_name_lower.contains(".ir_bench.")
        {
            let dest = Path::new(&config.iracing_folder).join(&file_name);
            if let Some(parent) = dest.parent()
            {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, dest)?;
        }
    }
    Ok(())
}

fn revert_from_backup_iracing_ini_files(config: &ConfigData, backup_folder: &Path) -> Result<()>
{
    if !backup_folder.exists()
    {
        return Ok(());
    }
    for entry in fs::read_dir(backup_folder)?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file()
        {
            continue;
        }
        let file_name = match path.file_name()
        {
            Some(name) => name.to_owned(),
            None => continue,
        };
        let dest = Path::new(&config.iracing_folder).join(file_name);
        if let Some(parent) = dest.parent()
        {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&path, dest)?;
    }
    Ok(())
}

fn copy_current_iracing_ini_files_to_folder(config: &ConfigData, target_folder: &Path)
-> Result<()>
{
    if config.iracing_folder.is_empty()
    {
        return Err(anyhow!("iRacing folder not configured"));
    }

    fs::create_dir_all(target_folder)?;

    for entry in fs::read_dir(&config.iracing_folder)?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file()
        {
            continue;
        }
        let file_name = match path.file_name()
        {
            Some(name) => name.to_owned(),
            None => continue,
        };
        let file_name_lower = file_name.to_string_lossy().to_lowercase();
        if file_name_lower.ends_with(".ini") || file_name_lower.contains(".ir_bench.")
        {
            let dest = target_folder.join(&file_name);
            if let Some(parent) = dest.parent()
            {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, dest)?;
        }
    }

    Ok(())
}

fn loop_get_data(connection: &mut Connection) -> Option<IracingSessionTelemetry>
{
    // Try to read a telemetry sample
    let sample = connection.telemetry().ok()?;

    let speed_mps = sample
        .get("Speed")
        .ok()
        .and_then(|v| value_to_f64(&v))
        .unwrap_or(0.0) as f32;

    let replay_frame_num = sample
        .get("ReplayFrameNum")
        .ok()
        .and_then(|v| value_to_i32(&v))
        .unwrap_or(0);

    let cam_car_idx = sample
        .get("CamCarIdx")
        .ok()
        .and_then(|v| value_to_i32(&v))
        .unwrap_or(0);

    let session_time = sample
        .get("SessionTime")
        .ok()
        .and_then(|v| value_to_f64(&v))
        .unwrap_or(0.0);

    let replay_frame_num_end = sample
        .get("ReplayFrameNumEnd")
        .ok()
        .and_then(|v| value_to_i32(&v))
        .unwrap_or(i32::MAX);

    Some(IracingSessionTelemetry {
        speed_mps,
        replay_frame_num,
        cam_car_idx,
        session_time,
        replay_frame_num_end,
    })
}

fn get_cancel_state(ui_state: &mut UiState) -> bool
{
    let cancel_state = ui_state.stop_flag.load(std::sync::atomic::Ordering::SeqCst);

    if cancel_state
    {
        ui_state
            .log_tx
            .send(format!("Get cancel state: {}", cancel_state))
            .ok();
    }

    cancel_state
}

fn check_canceled(ui_state: &mut UiState, sleep_s: f32) -> Result<()>
{
    let is_canceled = get_cancel_state(ui_state);
    // let ret_bench_state = BenchState {
    //     is_connected: bench_state.is_connected,
    //     is_running:   bench_state.is_running,
    //     is_at_end:    bench_state.is_at_end,
    // };

    if is_canceled
    {
        return Err(anyhow!("Benchmark canceled by user"));
    }

    if sleep_s > 0.0
    {
        std::thread::sleep(std::time::Duration::from_secs_f32(sleep_s));
    }

    Ok(())
}

fn single_bench_run_setup(
    ui_state: &mut UiState,
    connection: &mut Connection,
    config: &ConfigData,
) -> Result<()>
{
    const SETUP_INTERVAL_S: f32 = 1.0 / 4.0;

    replay_set_play_speed(0, false);
    check_canceled(ui_state, SETUP_INTERVAL_S)?;

    replay_search(RpySrchMode::ToStart);
    check_canceled(ui_state, SETUP_INTERVAL_S)?;

    replay_set_play_speed(1, false);
    check_canceled(ui_state, SETUP_INTERVAL_S)?;

    replay_set_play_speed(0, false);
    check_canceled(ui_state, SETUP_INTERVAL_S)?;

    replay_search(RpySrchMode::NextLap);
    check_canceled(ui_state, SETUP_INTERVAL_S)?;

    cam_set_state(CameraState::UI_HIDDEN);
    check_canceled(ui_state, SETUP_INTERVAL_S)?;

    replay_set_play_speed(config.play_speed, false);
    check_canceled(ui_state, SETUP_INTERVAL_S)?;

    let data = loop_get_data(connection)
        .ok_or_else(|| anyhow!("Unable to read iRacing telemetry for single run setup"))?;

    if config.verbose
    {
        println!("Initial Replay Frame Number: {}", data.replay_frame_num);
        println!("Initial Session Time: {}", data.session_time);
        println!("Initial Cam Car Index: {}", data.cam_car_idx);
        println!("Initial Speed: {:.0} km/h", data.speed_mps as f64 * 3.6);
    }

    if data.replay_frame_num_end <= 1
    {
        replay_set_play_speed(0, false);
    }

    Ok(())
}

pub fn run_benchmark_handler(ui_state: &mut UiState, cfg: &mut ConfigData)
{
    match run_benchmark_pipeline(ui_state, cfg)
    {
        Ok(()) =>
        {
            ui_state
                .log_tx
                .send("Benchmark pipeline completed successfully".to_string())
                .ok();
        }
        Err(e) =>
        {
            ui_state
                .log_tx
                .send(format!("Benchmark pipeline failed: {}", e))
                .ok();
        }
    }

    ui_state.log_tx.send("STATUS:Ready".to_string()).ok();
}

fn run_benchmark_pipeline(ui_state: &mut UiState, cfg: &mut ConfigData) -> Result<()>
{
    ui_state.log_tx.send("Benchmark started".to_string()).ok();

    if cfg.benchmark_program.is_empty()
    {
        return Err(anyhow!("Benchmark program not configured"));
    }

    if cfg.iracing_folder.is_empty()
    {
        return Err(anyhow!("iRacing folder not configured"));
    }

    if cfg.output_folder.is_empty()
    {
        return Err(anyhow!("Output folder not configured"));
    }

    ui_state
        .log_tx
        .send(format!("PROGRESS:{}/{}", 0, cfg.num_runs))
        .ok();

    let benchmark_input_path = Path::new(&cfg.input_file);
    let order_file = if !cfg.input_file.is_empty()
    {
        load_benchmark_order_file(benchmark_input_path).ok()
    }
    else
    {
        None
    };

    if let Some(order_file) = order_file
    {
        let input_folder = benchmark_input_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let current_bench_id_path = if cfg.current_bench_id_file.is_empty()
        {
            None
        }
        else
        {
            Some(resolve_relative_path(
                &input_folder,
                &cfg.current_bench_id_file,
            ))
        };

        let id_override = if cfg.current_bench_id_override > 0
        {
            Some(cfg.current_bench_id_override)
        }
        else
        {
            None
        };

        if let Some(override_id) = id_override
            && let Some(path) = current_bench_id_path.as_ref()
        {
            match write_current_bench_id(path, override_id)
            {
                Ok(_) =>
                {
                    ui_state
                        .log_tx
                        .send(format!("Overriding current bench id to {}", override_id))
                        .ok();
                }
                Err(e) =>
                {
                    ui_state
                        .log_tx
                        .send(format!("Warning: failed writing current bench id: {}", e))
                        .ok();
                }
            }
        }

        let current_bench_id = if let Some(path) = current_bench_id_path.as_deref()
        {
            read_current_bench_id(path).unwrap_or(-1)
        }
        else
        {
            -1
        };

        let mut benchmarks = order_file.benchmarks;
        benchmarks.sort_by_key(|entry| entry.id);

        if benchmarks.is_empty()
        {
            ui_state
                .log_tx
                .send("No benchmarks found in the order file".to_string())
                .ok();

            return Err(anyhow!("No benchmarks found in the order file"));
        }

        let backup_pathbuf = Path::new(&cfg.iracing_folder).join(".backup");
        let backup_root = match backup_iracing_ini_files(cfg, backup_pathbuf.as_path())
        {
            Ok(path) => path,
            Err(error) =>
            {
                println!("Failed to back up iRacing ini files: {error}");
                return Err(anyhow!(format!(
                    "Failed to back up iRacing ini files: {}",
                    error
                )));
            }
        };

        let bench_timestamp_str = Local::now().format("%Y-%m-%d_%H.%M.%S").to_string();

        for bench in benchmarks
        {
            ui_state
                .log_tx
                .send(format!("PROGRESS:{}/{}", 0, cfg.num_runs))
                .ok();

            if bench.id <= current_bench_id
            {
                continue;
            }

            if get_cancel_state(ui_state)
            {
                ui_state.log_tx.send("STATUS:Ready".to_string()).ok();

                return Err(anyhow!(format!(
                    "Benchmark canceled by user before starting benchmark {}",
                    bench.id
                )));
            }

            let bench_folder_name = Path::new(&bench.path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&bench.path)
                .to_string();
            let bench_folder = input_folder.join(&bench_folder_name);

            if !bench_folder.is_dir()
            {
                ui_state
                    .log_tx
                    .send(format!(
                        "Benchmark input folder not found: {}",
                        bench_folder.display()
                    ))
                    .ok();

                continue;
            }

            if let Err(e) = copy_bench_files_to_iracing_folder(cfg, &bench_folder)
            {
                ui_state.log_tx.send("STATUS:Ready".to_string()).ok();

                return Err(anyhow!(format!("Failed to copy benchmark files: {}", e)));
            }

            stop_bench_program(ui_state, cfg, None).ok();

            if let Ok(mut connection) = in_sim_benchmark_setup(ui_state, cfg)
            {
                ui_state
                    .log_tx
                    .send(format!(
                        "Benchmark {} setup completed, starting runs...",
                        bench.id
                    ))
                    .ok();

                let track_name =
                    get_track(&mut connection).unwrap_or_else(|_| "Unknown Track".to_string());
                let group_name = track_name;
                let group_path = Path::new(&cfg.output_folder)
                    .join(group_name)
                    .join(&bench_timestamp_str);

                if let Err(e) = fs::create_dir_all(&group_path)
                {
                    return Err(anyhow!(format!(
                        "Failed to create output group folder: {}",
                        e
                    )));
                }

                let timestamp_str = Local::now().format("%Y-%m-%d_%H.%M.%S").to_string();
                let benchmark_output_folder =
                    group_path.join(format!("{} {}", timestamp_str, bench_folder_name));

                if let Err(e) =
                    copy_current_iracing_ini_files_to_folder(cfg, &benchmark_output_folder)
                {
                    return Err(anyhow!(format!(
                        "Warning: failed copying iRacing INI files: {}",
                        e
                    )));
                }
                let note = format!("Created by ir-bench-gui at {}", Local::now().to_rfc3339());
                fs::write(benchmark_output_folder.join("comments.ir_bench.md"), note).ok();

                if let Err(e) = fs::create_dir_all(&benchmark_output_folder)
                {
                    ui_state.log_tx.send("STATUS:Ready".to_string()).ok();

                    return Err(anyhow!(format!(
                        "Failed to create benchmark output folder: {}",
                        e
                    )));
                }

                let runs_folder = benchmark_output_folder.join(&cfg.bench_run_folder_name);

                if let Err(e) = fs::create_dir_all(&runs_folder)
                {
                    ui_state.log_tx.send("STATUS:Ready".to_string()).ok();

                    return Err(anyhow!(format!(
                        "Failed to create benchmark runs output folder: {}",
                        e
                    )));
                }

                for run in 1..=cfg.num_runs
                {
                    if get_cancel_state(ui_state)
                    {
                        reset_current_bench_id_file(cfg).ok();

                        ui_state.log_tx.send("STATUS:Ready".to_string()).ok();

                        return Err(anyhow!("Benchmark canceled by user"));
                    }

                    ui_state
                        .log_tx
                        .send(format!(
                            "Starting benchmark {} run {}/{}...",
                            bench.id, run, cfg.num_runs
                        ))
                        .ok();

                    let timestamp_str = Local::now().format("%Y-%m-%d_%H.%M.%S").to_string();
                    let output_name =
                        format!("{}_run-{}-of-{}.csv", timestamp_str, run, cfg.num_runs);
                    let output_path = runs_folder.join(output_name);

                    match in_sim_benchmark_loop(ui_state, cfg, &mut connection, &output_path)
                    {
                        Ok(()) =>
                        {
                            stop_bench_program(ui_state, cfg, Some(&output_path)).ok();

                            ui_state
                                .log_tx
                                .send(format!("PROGRESS:{}/{}", run, cfg.num_runs))
                                .ok();
                        }
                        Err(e) =>
                        {
                            stop_bench_program(ui_state, cfg, Some(&output_path)).ok();
                            reset_current_bench_id_file(cfg).ok();

                            ui_state.log_tx.send("STATUS:Ready".to_string()).ok();

                            return Err(anyhow!(format!(
                                "Benchmark {} run {} failed: {}",
                                bench.id, run, e
                            )));
                        }
                    }

                    if ui_state.stop_after_run_flag.swap(false, Ordering::SeqCst)
                    {
                        reset_current_bench_id_file(cfg).ok();

                        ui_state.log_tx.send("STATUS:Ready".to_string()).ok();

                        return Err(anyhow!(format!(
                            "Stop requested after current run for benchmark {}",
                            bench.id
                        )));
                    }

                    std::thread::sleep(std::time::Duration::from_millis(100));
                }

                if let Some(path) = current_bench_id_path.as_ref()
                    && let Err(e) = write_current_bench_id(path, bench.id)
                {
                    ui_state
                        .log_tx
                        .send(format!("Warning: failed writing current bench id: {}", e))
                        .ok();
                }

                if ui_state
                    .stop_after_last_run_flag
                    .swap(false, Ordering::SeqCst)
                {
                    reset_current_bench_id_file(cfg).ok();

                    ui_state.log_tx.send("STATUS:Ready".to_string()).ok();

                    return Err(anyhow!(
                        "Stop requested after the last run of the current benchmark"
                    ));
                }

                // update the current Bench Visualization input folder in the config to the latest
                // benchmark output folder
                let latest_bench_vis_input_folder = group_path.to_string_lossy().to_string();
                cfg.bench_vis_input_folder = latest_bench_vis_input_folder.clone();

                match wait_for_iracing_disconnection(ui_state, cfg.connection_timeout)
                {
                    Ok(()) =>
                    {
                        stop_connection_timer(ui_state);
                    }
                    Err(e) =>
                    {
                        ui_state
                            .log_tx
                            .send(format!("Error waiting for iRacing disconnection: {}", e))
                            .ok();

                        reset_current_bench_id_file(cfg).ok();

                        let group_path = Path::new(&cfg.bench_vis_input_folder);
                        open_bench_vis_exe(ui_state, group_path, &cfg.bench_run_folder_name).ok();

                        ui_state.log_tx.send("STATUS:Ready".to_string()).ok();

                        return Err(anyhow!(format!("Benchmark {} failed", bench.id)));
                    }
                }
            }
            else
            {
                reset_current_bench_id_file(cfg).ok();

                let group_path = Path::new(&cfg.bench_vis_input_folder);
                open_bench_vis_exe(ui_state, group_path, &cfg.bench_run_folder_name).ok();

                ui_state.log_tx.send("STATUS:Ready".to_string()).ok();

                return Err(anyhow!(format!("Benchmark {} failed", bench.id)));
            }
        }

        ui_state
            .log_tx
            .send("All ordered benchmark runs completed".to_string())
            .ok();

        reset_current_bench_id_file(cfg).ok();
        revert_from_backup_iracing_ini_files(cfg, &backup_root).ok();

        let group_path = Path::new(&cfg.bench_vis_input_folder);
        open_bench_vis_exe(ui_state, group_path, &cfg.bench_run_folder_name).ok();
    }

    // open_bench_vis_exe(ui_state, &group_path)
    Ok(())
}

pub fn open_bench_vis_exe(
    ui_state: &mut UiState,
    group_path: &Path,
    runs_folder_name: &str,
) -> Result<()>
{
    let bin_folder_path = get_app_root_dir();
    let bench_vis_exe = bin_folder_path.join("bench-vis");

    ui_state
        .log_tx
        .send(format!(
            "Launching bench-vis: {} '{}'",
            bench_vis_exe.display(),
            group_path.display()
        ))
        .ok();

    let mut cmd = Command::new(bench_vis_exe);

    let mut child = cmd
        .args([group_path.as_os_str(), runs_folder_name.as_ref()])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("Failed to launch bench-vis: {}", e))?;

    if let Some(stderr) = child.stderr.take()
    {
        let tx = ui_state.log_tx.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok)
            {
                tx.send(format!("[bench-vis stderr] {line}")).ok();
            }
        });
    }

    if let Some(stdout) = child.stdout.take()
    {
        let tx = ui_state.log_tx.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok)
            {
                tx.send(format!("[bench-vis stdout] {line}")).ok();
            }
        });
    }

    match child.try_wait()
    {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow!("Failed waiting for bench-vis: {}", e)),
    }
}

fn reset_current_bench_id_file(cfg: &ConfigData) -> Result<()>
{
    if cfg.current_bench_id_file.is_empty()
    {
        return Ok(());
    }

    let current_bench_id_path = Path::new(&cfg.current_bench_id_file);
    if current_bench_id_path.exists()
    {
        fs::write(current_bench_id_path, "0")?;
    }

    Ok(())
}

fn in_sim_benchmark_setup(ui_state: &mut UiState, cfg: &ConfigData) -> Result<Connection>
{
    let (connection, track, sim_mode) =
        match wait_for_iracing_replay_connection(ui_state, cfg.connection_timeout)
        {
            Ok(result) => result,
            Err(err) =>
            {
                return Err(anyhow!(err));
            }
        };

    ui_state
        .log_tx
        .send(format!(
            "Starting in-sim benchmark on {} ({})",
            track, sim_mode
        ))
        .ok();

    Ok(connection)
}

fn get_track(connection: &mut Connection) -> Result<String>
{
    let session_info = get_iracing_session_info(connection)?;
    let track_name = session_info.weekend.track_name.clone();

    Ok(track_name)
}

fn in_sim_benchmark_loop(
    ui_state: &mut UiState,
    cfg: &ConfigData,
    connection: &mut Connection,
    output_path: &Path,
) -> Result<()>
{
    single_bench_run_setup(ui_state, connection, cfg)?;
    match start_bench_program(ui_state, cfg, output_path)
    {
        Ok(()) =>
        {}
        Err(e) => return Err(anyhow!("Failed to start benchmark program: {}", e)),
    }

    loop
    {
        if get_cancel_state(ui_state)
        {
            ui_state
                .log_tx
                .send("Benchmark canceled by user".to_string())
                .ok();

            stop_bench_program(ui_state, cfg, Some(output_path)).ok();

            return Err(anyhow!("Benchmark canceled by user"));
        }

        if let Some(data) = loop_get_data(connection)
        {
            if cfg.verbose
            {
                ui_state
                    .log_tx
                    .send(format!(
                        "Replay Frame Number: {} | Session Time: {} | Cam Car Index: {} | Speed: {:.0} km/h",
                        data.replay_frame_num,
                        data.session_time,
                        data.cam_car_idx,
                        data.speed_mps as f64 * 3.6
                    ))
                    .ok();
            }

            if data.replay_frame_num_end <= 1
            {
                replay_set_play_speed(0, false);
                check_canceled(ui_state, 1.0 / 60.0)?;

                cam_set_state(CameraState::empty());
                check_canceled(ui_state, 1.0 / 60.0)?;

                stop_bench_program(ui_state, cfg, Some(output_path)).ok();

                return Ok(());
            }
        }

        thread::sleep(StdDuration::from_secs_f32(0.25));
    }
}

fn start_bench_program(ui_state: &mut UiState, cfg: &ConfigData, output_path: &Path) -> Result<()>
{
    let process_name = if cfg.process_name.is_empty()
    {
        "iRacingSim64DX11"
    }
    else
    {
        &cfg.process_name
    };

    let binding = output_path.display().to_string();
    let args_str = [
        "--process_name",
        process_name,
        "--output_file",
        binding.as_str(),
    ];

    let display_args_str = args_str.join(" ");

    ui_state
        .log_tx
        .send(format!(
            "Launching benchmark program: {} {}",
            cfg.benchmark_program, display_args_str
        ))
        .ok();

    let mut child_cmd = Command::new(&cfg.benchmark_program);
    child_cmd.args(args_str);
    child_cmd.stderr(Stdio::piped());

    let mut child = child_cmd
        .spawn()
        .map_err(|e| anyhow!("Failed to start benchmark process: {e}"))?;

    if let Some(stderr) = child.stderr.take()
    {
        let tx = ui_state.log_tx.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok)
            {
                tx.send(format!("[benchmark stderr] {line}")).ok();
            }
        });
    }

    Ok(())
}

fn stop_bench_program(
    ui_state: &mut UiState,
    cfg: &ConfigData,
    output_path: Option<&Path>,
) -> Result<()>
{
    let output_str = output_path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let stop_arg = cfg.benchmark_terminate_args.join(" ");
    let process_name = cfg.process_name.clone();
    let args_str = ["--process_name", process_name.as_str(), stop_arg.as_str()];
    // let args_str = [stop_arg];
    let display_args_str = args_str.join(" ");

    ui_state
        .log_tx
        .send(format!(
            "Launching benchmark program: {} {}",
            cfg.benchmark_program, display_args_str
        ))
        .ok();

    let benchmark_program_path = Path::new(&cfg.benchmark_program);

    // stop the benchmark process by calling the program again with the terminate flag
    let mut stop_cmd = Command::new(benchmark_program_path);
    let mut child_stop_cmd = stop_cmd
        .args(args_str)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("Failed to stop benchmark process: {e}"))?;

    match child_stop_cmd.wait()
    {
        Ok(status) =>
        {
            if !status.success()
            {
                Err(anyhow!(
                    "Failed to stop benchmark process, exit code: {}",
                    status.code().unwrap_or(-1)
                ))
            }
            else
            {
                ui_state
                    .log_tx
                    .send(format!("Benchmark output written to {}", output_str))
                    .ok();

                Ok(())
            }
        }
        Err(e) => Err(anyhow!("Failed to wait for benchmark stop process: {e}")),
    }
}

fn stop_connection_timer(ui_state: &mut UiState)
{
    ui_state
        .log_tx
        .send("TIMEOUT_REMAINING:-:-:0.0".to_string())
        .ok();
}

fn update_connection_timer(
    ui_state: &mut UiState,
    start_time: std::time::Instant,
    timeout: std::time::Duration,
) -> bool
{
    let time_elapsed = std::time::Instant::now().duration_since(start_time);
    let time_remaining = (timeout - time_elapsed).as_secs();
    let timer_running = time_remaining > 0;
    let cancel_state = get_cancel_state(ui_state);

    if cancel_state || !timer_running
    {
        stop_connection_timer(ui_state);

        return false;
    }

    let mins = time_remaining / 60;
    let secs = time_remaining % 60;
    let timeout_percentage = 1.0 - (time_remaining as f32 / timeout.as_secs_f32());

    let timer_display = format!("{}:{:02}:{:.2}", mins, secs, timeout_percentage);
    let timer_str = format!("TIMEOUT_REMAINING:{}", timer_display);

    ui_state.log_tx.send(timer_str).ok();

    timer_running
}

fn wait_for_iracing_disconnection(ui_state: &mut UiState, timeout_secs: i32) -> Result<()>
{
    ui_state
        .log_tx
        .send("Waiting for iRacing disconnection...".to_string())
        .ok();

    let timeout = std::time::Duration::from_secs_f64(timeout_secs.max(0) as f64);
    let start_time = std::time::Instant::now();

    loop
    {
        if !update_connection_timer(ui_state, start_time, timeout)
        {
            return Err(anyhow!(
                "Timeout reached while waiting for iRacing disconnection"
            ));
        }

        let elapsed = std::time::Instant::now().duration_since(start_time);
        let time_remaining = timeout.checked_sub(elapsed).unwrap_or_default();

        if time_remaining.is_zero()
        {
            return Err(anyhow!(
                "Timeout reached while waiting for iRacing disconnection"
            ));
        }

        if get_cancel_state(ui_state)
        {
            return Err(anyhow!(
                "Benchmark canceled by user while waiting for iRacing disconnection"
            ));
        }

        let blocking_conn = Connection::new()?.blocking();
        match blocking_conn
        {
            Ok(bc) =>
            {
                // Try to read a telemetry sample with a timeout of 1 second
                let sample_result = bc.sample(std::time::Duration::from_secs(1));
                if sample_result.is_err()
                {
                    ui_state
                        .log_tx
                        .send("iRacing disconnected!".to_string())
                        .ok();

                    stop_connection_timer(ui_state);

                    bc.close().ok();
                    return Ok(());
                }
            }
            Err(_) =>
            {
                ui_state
                    .log_tx
                    .send("iRacing disconnected!".to_string())
                    .ok();

                stop_connection_timer(ui_state);

                return Ok(());
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1_000));
    }
}

fn wait_for_iracing_replay_connection(
    ui_state: &mut UiState,
    timeout_secs: i32,
) -> Result<(Connection, String, String)>
{
    let timeout = std::time::Duration::from_secs_f64(timeout_secs.max(0) as f64);
    let start_time = std::time::Instant::now();

    ui_state
        .log_tx
        .send("Waiting for iRacing replay connection...".to_string())
        .ok();

    // let mut must_sleep = false;
    // loop until we get a valid connection and valid new telemetry sample, or until timeout is
    // reached
    loop
    {
        if !update_connection_timer(ui_state, start_time, timeout)
        {
            return Err(anyhow!(
                "Timeout reached while waiting for iRacing replay connection"
            ));
        }

        let elapsed = std::time::Instant::now().duration_since(start_time);
        let time_remaining = if let Some(remaining) = timeout.checked_sub(elapsed)
        {
            remaining
        }
        else
        {
            std::time::Duration::from_secs(0)
        };

        if time_remaining.is_zero()
        {
            return Err(anyhow!(
                "Timeout reached while waiting for iRacing replay connection"
            ));
        }

        // if must_sleep
        // {
        //     std::thread::sleep(std::time::Duration::from_millis(1_000));
        // }

        if let Ok(mut connection) = get_iracing_connection()
            && let Ok(sample) = get_iracing_telemetry_sample(&mut connection)
            && let Ok(session) = get_iracing_session_info(&mut connection)
        {
            replay_search(RpySrchMode::ToStart);
            match sample.get("ReplayFrameNum")
            {
                Ok(Value::INT(replay_frame_num)) if (0..=1).contains(&replay_frame_num) =>
                {
                    let weekend = session.weekend;
                    let track = weekend.track_display_name.clone();

                    stop_connection_timer(ui_state);

                    return Ok((connection, track, "replay".to_string()));
                }
                _ =>
                {}
            }
        }

        std::thread::sleep(std::time::Duration::from_secs_f32(1.0 / 60.0));
    }
}

fn get_iracing_connection() -> Result<Connection>
{
    let conn = Connection::new().map_err(|e| anyhow!("Unable to open telemetry: {}", e))?;
    Ok(conn)
}

fn get_iracing_telemetry_sample(conn: &mut Connection) -> Result<Sample>
{
    let sample = conn
        .telemetry()
        .map_err(|e| anyhow!("Unable to read telemetry sample: {}", e))?;
    Ok(sample)
}

fn get_iracing_session_info(conn: &mut Connection) -> Result<SessionDetails>
{
    let session_info = conn
        .session_info()
        .map_err(|e| anyhow!("Unable to read session info: {}", e))?;
    Ok(session_info)
}
