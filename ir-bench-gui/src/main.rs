// Prevent console window in addition to Slint window in Windows release builds when, e.g., starting
// the app via file manager. Ignored on other platforms.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    error::Error,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use anyhow::Result;
use serde::{Deserialize, Serialize};
mod bench_ops;
use bench_ops::{open_bench_vis_exe, run_benchmark_handler};
use slint::ToSharedString;

slint::include_modules!();

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConfigData
{
    output_folder:             String,
    bench_vis_input_folder:    String,
    bench_run_folder_name:     String,
    #[serde(rename = "benchmark_input_file")]
    input_file:                String,
    current_bench_id_file:     String,
    current_bench_id_override: i32,
    benchmark_program:         String,
    sim_name:                  String,
    process_name:              String,
    iracing_folder:            String,
    num_runs:                  i32,
    play_speed:                i32,
    connection_timeout:        i32,
    verbose:                   bool,
    #[serde(default)]
    benchmark_terminate_args:  Vec<String>,
}

#[derive(Clone)]
struct UiState
{
    ui_handle:                slint::Weak<AppWindow>,
    log_rx:                   Arc<Mutex<mpsc::Receiver<String>>>,
    log_tx:                   mpsc::Sender<String>,
    stop_flag:                Arc<AtomicBool>,
    stop_after_run_flag:      Arc<AtomicBool>,
    stop_after_last_run_flag: Arc<AtomicBool>,
}

impl Default for ConfigData
{
    fn default() -> Self
    {
        Self {
            output_folder:             String::new(),
            bench_vis_input_folder:    String::new(),
            bench_run_folder_name:     String::new(),
            input_file:                String::new(),
            current_bench_id_file:     String::new(),
            current_bench_id_override: 0,
            benchmark_program:         String::new(),
            sim_name:                  String::new(),
            process_name:              String::new(),
            iracing_folder:            String::new(),
            num_runs:                  3,
            play_speed:                1,
            connection_timeout:        120,
            verbose:                   false,
            benchmark_terminate_args:  vec!["--terminate_existing_session".to_string()],
        }
    }
}

fn get_app_root_dir() -> PathBuf
{
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn get_config_path() -> PathBuf { get_app_root_dir().join("config/config.json5") }

fn read_config() -> Result<ConfigData>
{
    let config_path = get_config_path();
    if !config_path.is_file()
    {
        return Ok(ConfigData::default());
    }
    let content = fs::read_to_string(&config_path)?;
    let cfg = json5::from_str(&content);

    match cfg
    {
        Ok(cfg) =>
        {
            let mut cfg = cfg;
            sanitize_config_paths(&mut cfg);
            Ok(cfg)
        }
        Err(e) => Err(anyhow::anyhow!("Failed to parse config file: {}", e)),
    }
}

fn get_absolute_path(path: &str) -> Option<PathBuf>
{
    if path.is_empty()
    {
        return None;
    }

    let path = PathBuf::from(path);
    if path.is_absolute()
    {
        Some(path)
    }
    else
    {
        Some(get_app_root_dir().join(path))
    }
}

fn absolute_path_str_to_string(path: &str) -> String
{
    match get_absolute_path(path)
    {
        Some(abs_path) => abs_path.to_str().unwrap_or("").to_string(),
        None => String::new(),
    }
}

fn sanitize_config_paths(cfg: &mut ConfigData)
{
    cfg.output_folder = absolute_path_str_to_string(&cfg.output_folder);
    cfg.bench_vis_input_folder = absolute_path_str_to_string(&cfg.bench_vis_input_folder);
    cfg.input_file = absolute_path_str_to_string(&cfg.input_file);
    cfg.current_bench_id_file = absolute_path_str_to_string(&cfg.current_bench_id_file);
    cfg.benchmark_program = absolute_path_str_to_string(&cfg.benchmark_program);
    cfg.iracing_folder = absolute_path_str_to_string(&cfg.iracing_folder);
}

fn save_config(config: &ConfigData) -> Result<()>
{
    let config_path = get_config_path();
    // Prefer writing JSON5 when possible to maintain round-trip compatibility
    // with human-edited `config.json5` files. Fall back to JSON if the
    // `json5` crate does not support serialization.
    let json = match json5::to_string(&config)
    {
        Ok(s) => s,
        Err(_) => serde_json::to_string_pretty(&config)?,
    };

    match fs::write(&config_path, json)
    {
        Ok(_) => Ok(()),
        Err(e) => Err(anyhow::anyhow!("Failed to write config file: {}", e)),
    }
}

fn main() -> Result<(), Box<dyn Error>>
{
    let ui = AppWindow::new()?;

    // Log channel used by background worker threads
    let (log_tx, log_rx) = mpsc::channel::<String>();
    let log_rx = Arc::new(Mutex::new(log_rx));
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_after_run_flag = Arc::new(AtomicBool::new(false));
    let stop_after_last_run_flag = Arc::new(AtomicBool::new(false));

    // Call the load config handler to initialize the UI with the current config
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        load_config_handler(&mut ui_state.clone());
    }

    // Load config handler
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_load_config(move || load_config_handler(&mut ui_state.clone()));
    }

    // Save config handler
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_save_config(move || save_config_handler(&mut ui_state.clone()));
    }

    // Browse output folder handler
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_browse_output_folder(move || browse_output_folder_handler(ui_state.clone()));
    }

    // Browse input file handler
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_browse_input_file(move || browse_input_file_handler(ui_state.clone()));
    }

    // Browse bench folder handler
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_browse_bench_folder(move || browse_bench_folder_handler(ui_state.clone()));
    }

    // Start benchmark handler
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_start_benchmark(move || start_benchmark_handler(&mut ui_state.clone()));
    }

    // Stop benchmark handlers
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_stop_benchmark(move || stop_benchmark_handler(&mut ui_state.clone()));
    }

    // Stop benchmark handler: Stop Now
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_stop_now(move || stop_now_handler(&mut ui_state.clone()));
    }

    // Stop benchmark handler: Stop Now
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_stop_now(move || stop_now_handler(&mut ui_state.clone()));
    }

    // Stop benchmark handler: Stop After Run
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_stop_after_run(move || stop_after_run_handler(&mut ui_state.clone()));
    }

    // Stop benchmark handler: Stop After Last Run
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_stop_after_last_run(move || stop_after_last_run_handler(&mut ui_state.clone()));
    }

    // Stop benchmark handler: Stop After Last Run
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_stop_after_last_run(move || stop_after_last_run_handler(&mut ui_state.clone()));
    }

    // Open results folder in platform file explorer
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_open_results(move || open_results_folder_handler(ui_state.clone()));
    }

    // Launch bench-vis for the configured output folder (runs `cargo run -p bench-vis -- <out>`)
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_open_bench_vis(move || open_bench_vis_handler(ui_state.clone()));
    }

    // Poll messages from background threads on the UI thread using the
    // `poll_messages` callback (triggered by a Slint Timer in the UI).
    {
        let ui_handle = ui.as_weak();
        let ui_state: UiState = UiState {
            ui_handle:                ui_handle.clone(),
            log_rx:                   log_rx.clone(),
            log_tx:                   log_tx.clone(),
            stop_flag:                stop_flag.clone(),
            stop_after_run_flag:      stop_after_run_flag.clone(),
            stop_after_last_run_flag: stop_after_last_run_flag.clone(),
        };

        ui.on_poll_messages(move || poll_msgs(ui_state.clone()));
    }

    ui.run()?;

    Ok(())
}

fn load_config(ui_state: &mut UiState, cfg: &ConfigData)
{
    let ui = ui_state.ui_handle.unwrap();

    ui.set_output_folder(cfg.output_folder.to_shared_string());
    ui.set_bench_vis_input_folder(cfg.bench_vis_input_folder.to_shared_string());
    ui.set_bench_run_folder_name(cfg.bench_run_folder_name.to_shared_string());
    ui.set_benchmark_input_file(cfg.input_file.to_shared_string());
    ui.set_current_bench_id_file(cfg.current_bench_id_file.to_shared_string());
    ui.set_current_bench_id_override(cfg.current_bench_id_override);
    ui.set_benchmark_program(cfg.benchmark_program.to_shared_string());
    ui.set_sim_name(cfg.sim_name.to_shared_string());
    ui.set_process_name(cfg.process_name.to_shared_string());
    ui.set_iracing_folder(cfg.iracing_folder.to_shared_string());
    ui.set_num_runs(cfg.num_runs);
    ui.set_play_speed(cfg.play_speed);
    ui.set_connection_timeout(cfg.connection_timeout);
    ui.set_verbose(cfg.verbose);
    ui.set_benchmark_terminate_args(cfg.benchmark_terminate_args.join(", ").to_shared_string());
}

fn load_config_handler(ui_state: &mut UiState)
{
    let cfg = read_config();
    ui_state
        .log_tx
        .send("Config load requested".to_string())
        .ok();

    match cfg
    {
        Ok(cfg) =>
        {
            load_config(ui_state, &cfg);

            ui_state
                .log_tx
                .send("Config loaded successfully".to_string())
                .ok();
        }
        Err(e) =>
        {
            ui_state
                .log_tx
                .send(format!("Failed to load config: {}", e))
                .ok();
        }
    };
}

fn get_config_from_ui(ui_state: UiState) -> ConfigData
{
    let ui = ui_state.ui_handle.unwrap();

    let cfg: ConfigData = ConfigData {
        output_folder:             ui.get_output_folder().to_string(),
        bench_vis_input_folder:    ui.get_bench_vis_input_folder().to_string(),
        bench_run_folder_name:     ui.get_bench_run_folder_name().to_string(),
        input_file:                ui.get_benchmark_input_file().to_string(),
        current_bench_id_file:     ui.get_current_bench_id_file().to_string(),
        current_bench_id_override: ui.get_current_bench_id_override(),
        benchmark_program:         ui.get_benchmark_program().to_string(),
        sim_name:                  ui.get_sim_name().to_string(),
        process_name:              ui.get_process_name().to_string(),
        iracing_folder:            ui.get_iracing_folder().to_string(),
        num_runs:                  ui.get_num_runs(),
        play_speed:                ui.get_play_speed(),
        connection_timeout:        ui.get_connection_timeout(),
        verbose:                   ui.get_verbose().to_string().to_lowercase() == "true",
        benchmark_terminate_args:  json5::from_str(ui.get_benchmark_terminate_args().as_str())
            .unwrap_or(vec!["--terminate_existing_session".to_string()]),
    };

    cfg
}

fn save_config_handler(ui_state: &mut UiState)
{
    let cfg = get_config_from_ui(ui_state.clone());
    ui_state
        .log_tx
        .send("Config save requested".to_string())
        .ok();
    // ui_state.log_tx.send(format!("Config: {:?}", cfg)).ok();

    match save_config(&cfg)
    {
        Ok(_) => ui_state
            .log_tx
            .send("Config saved successfully".to_string())
            .ok(),

        Err(e) => ui_state
            .log_tx
            .send(format!("Failed to save config: {}", e))
            .ok(),
    };
}

fn browse_output_folder_handler(ui_state: UiState)
{
    let ui = ui_state.ui_handle.unwrap();
    let start_dir = ui.get_output_folder().to_string();
    let dialog = rfd::FileDialog::new();
    let dialog = if start_dir.is_empty()
    {
        dialog
    }
    else
    {
        dialog.set_directory(start_dir)
    };
    if let Some(folder) = dialog.pick_folder()
    {
        ui.set_output_folder(folder.display().to_shared_string());
    }
}

fn browse_input_file_handler(ui_state: UiState)
{
    let ui = ui_state.ui_handle.unwrap();
    let start_dir = ui.get_benchmark_input_file().to_string();
    let dialog = rfd::FileDialog::new();
    let dialog = if start_dir.is_empty()
    {
        dialog
    }
    else
    {
        dialog.set_directory(start_dir)
    };
    if let Some(file) = dialog.pick_file()
    {
        ui.set_benchmark_input_file(file.display().to_shared_string());
    }
}

fn browse_bench_folder_handler(ui_state: UiState)
{
    let ui = ui_state.ui_handle.unwrap();
    let start_dir = ui.get_iracing_folder().to_string();
    let dialog = rfd::FileDialog::new();
    let dialog = if start_dir.is_empty()
    {
        dialog
    }
    else
    {
        dialog.set_directory(start_dir)
    };
    if let Some(folder) = dialog.pick_folder()
    {
        ui.set_iracing_folder(folder.display().to_shared_string());
    }
}

fn start_benchmark_handler(ui_state: &mut UiState)
{
    let ui = ui_state.ui_handle.unwrap();
    ui.set_status("Starting...".to_shared_string());
    ui.set_connection_timeout_countdown("-".to_shared_string());
    ui_state.stop_flag.store(false, Ordering::SeqCst);
    ui_state.stop_after_run_flag.store(false, Ordering::SeqCst);
    ui_state
        .stop_after_last_run_flag
        .store(false, Ordering::SeqCst);

    let mut config: ConfigData = get_config_from_ui(ui_state.clone());

    // Run the benchmark in a separate thread to avoid blocking the UI
    std::thread::spawn({
        let mut ui_state = ui_state.clone();
        move || {
            run_benchmark_handler(&mut ui_state, &mut config);
        }
    });
}

fn stop_benchmark_handler(ui_state: &mut UiState)
{
    ui_state.stop_flag.store(true, Ordering::SeqCst);
    ui_state.stop_after_run_flag.store(false, Ordering::SeqCst);
    ui_state
        .stop_after_last_run_flag
        .store(false, Ordering::SeqCst);
    let ui = ui_state.ui_handle.unwrap();
    // ui.set_status("Stopping...".to_shared_string());
    ui.set_connection_timeout_countdown("-".to_shared_string());
    ui_state
        .log_tx
        .send("Stop requested from UI".to_string())
        .ok();
}

fn stop_now_handler(ui_state: &mut UiState)
{
    ui_state.stop_flag.store(true, Ordering::SeqCst);
    ui_state.stop_after_run_flag.store(false, Ordering::SeqCst);
    ui_state
        .stop_after_last_run_flag
        .store(false, Ordering::SeqCst);
    let ui = ui_state.ui_handle.unwrap();
    // ui.set_status("Stopping now...".to_shared_string());
    ui.set_connection_timeout_countdown("-".to_shared_string());
    ui_state.log_tx.send("Stop requested now".to_string()).ok();
}

fn stop_after_run_handler(ui_state: &mut UiState)
{
    ui_state.stop_flag.store(false, Ordering::SeqCst);
    ui_state.stop_after_run_flag.store(true, Ordering::SeqCst);
    ui_state
        .stop_after_last_run_flag
        .store(false, Ordering::SeqCst);
    // let ui = ui_state.ui_handle.unwrap();
    // ui.set_status("Stop after current run requested".to_shared_string());
    ui_state
        .log_tx
        .send("Stop requested after current run".to_string())
        .ok();
}

fn stop_after_last_run_handler(ui_state: &mut UiState)
{
    ui_state.stop_flag.store(false, Ordering::SeqCst);
    ui_state.stop_after_run_flag.store(false, Ordering::SeqCst);
    ui_state
        .stop_after_last_run_flag
        .store(true, Ordering::SeqCst);
    // let ui = ui_state.ui_handle.unwrap();
    // ui.set_status("Stop after last run requested".to_shared_string());
    ui_state
        .log_tx
        .send("Stop requested after last run".to_string())
        .ok();
}

fn open_results_folder_handler(ui_state: UiState)
{
    let ui = ui_state.ui_handle.unwrap();
    let out = ui.get_output_folder().to_string();
    if out.is_empty()
    {
        ui_state
            .log_tx
            .send("Output folder not configured".to_string())
            .ok();
        return;
    }
    if let Err(e) = open::that(&out)
    {
        ui_state
            .log_tx
            .send(format!("Failed to open results folder: {}", e))
            .ok();
    }
}

fn open_bench_vis_handler(ui_state: UiState)
{
    let ui = ui_state.ui_handle.unwrap();
    let out = ui.get_bench_vis_input_folder().to_string();
    let name = ui.get_bench_run_folder_name().to_string();
    let out_path = PathBuf::from(&out);
    if !out_path.is_dir()
    {
        ui_state
            .log_tx
            .send("Bench visualization input folder not configured or does not exist".to_string())
            .ok();

        return;
    }

    thread::spawn(move || {
        open_bench_vis_exe(&mut ui_state.clone(), &out_path.clone(), &name).ok();
    });
}

fn poll_msgs(ui_state: UiState)
{
    let ui = ui_state.ui_handle.unwrap();
    let mut prepend_logs = String::new();
    {
        let rx_lock = ui_state.log_rx.lock().unwrap();
        loop
        {
            match rx_lock.try_recv()
            {
                Ok(msg) =>
                {
                    if let Some(rest) = msg.strip_prefix("PROGRESS:")
                    {
                        let mut parts = rest.split('/');
                        if let (Some(cur), Some(total)) = (parts.next(), parts.next())
                        {
                            if let (Ok(cur_i), Ok(total_i)) =
                                (cur.parse::<i32>(), total.parse::<i32>())
                            {
                                ui.set_progress(cur_i);
                                ui.set_progress_total(total_i);
                                ui.set_status("Running".to_shared_string());
                            }
                            continue;
                        }
                    }

                    if let Some(rest) = msg.strip_prefix("STATUS:")
                    {
                        ui.set_status(rest.to_shared_string());
                        continue;
                    }

                    if let Some(rest) = msg.strip_prefix("TIMEOUT_REMAINING:")
                    {
                        // Expecting format "TIMEOUT_REMAINING:<minutes>:<seconds>:<percentage>"
                        let mut parts = rest.split(':');
                        if let (Some(min), Some(secs), Some(percentage)) =
                            (parts.next(), parts.next(), parts.next())
                        {
                            let display = match (
                                min.parse::<i64>(),
                                secs.parse::<i64>(),
                                percentage.parse::<f32>(),
                            )
                            {
                                (Ok(min_val), Ok(secs_val), Ok(percentage_val))
                                    if min_val >= 0
                                        && secs_val >= 0
                                        && (0.0..=1.0).contains(&percentage_val) =>
                                {
                                    ui.set_connection_timeout_percentage(percentage_val);
                                    format!("{}:{:02}", min_val, secs_val)
                                }
                                (Err(_), Err(_), Ok(percentage_val)) =>
                                {
                                    ui.set_connection_timeout_percentage(percentage_val);
                                    "-".to_string()
                                }
                                (Err(_), Err(_), Err(_)) =>
                                {
                                    ui.set_connection_timeout_percentage(0.0);
                                    "-".to_string()
                                }
                                _ => "-".to_string(),
                            };

                            ui.set_connection_timeout_countdown(display.to_shared_string());

                            continue;
                        }
                    }

                    let timestamp = chrono::Local::now().format("%H:%M:%S");
                    prepend_logs.insert_str(0, &format!("[ {} ]  {}\n\n", timestamp, msg));
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
    }

    if !prepend_logs.is_empty()
    {
        let mut current = ui.get_logs().to_string();
        current.insert_str(0, &prepend_logs);
        ui.set_logs(current.to_shared_string());
    }
}
