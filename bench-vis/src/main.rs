use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use csv::StringRecord;
use plotly::{
    Layout,
    Plot,
    Scatter,
    common::{DashType, Fill, Font, HoverInfo, Line, Mode},
    layout::{
        Annotation,
        Axis,
        HoverMode,
        RangeMode,
        Shape,
        ShapeLine,
        ShapeType,
        themes::PLOTLY_DARK,
    },
};
use rayon::prelude::*;
use serde_json::json;

const BIN_STEP: f64 = 0.1;
const TIME_BIN_PERCENT: f64 = 1.0;
const HZ_VALUES: [u32; 8] = [30, 60, 72, 80, 90, 120, 144, 240];
const HOVER_TITLE_EM_SIZE: f64 = 2.0;
const HIDDEN_UNIFIED_X_TITLE_HTML: &str = "<span style='font-size:0em;'>%{x}</span>";
const FRAME_TIME_PLOT_ID: &str = "frame_time_plot";
const FRAME_RATE_PLOT_ID: &str = "frame_rate_plot";

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct CliArgs
{
    /// Directory containing benchmark run subdirectories (e.g.
    /// "...\benchmarks\results\iRacing\charlotte 2025 roval2025\20-starters")
    benchmark_results_path: PathBuf,
    bench_run_folder_name:  String,
}

#[derive(Debug, Clone)]
struct RunData
{
    #[cfg(false)]
    file_name:     String,
    frametimes_ms: Vec<f64>,
}

#[derive(Debug, Clone)]
struct DistStats
{
    file_group_name: String,
    bin:             f64,
    mean:            f64,
    min:             f64,
    max:             f64,
}

#[derive(Debug, Clone)]
struct FpsOverTimeStats
{
    file_group_name: String,
    run_pct_bin:     f64,
    mean:            f64,
    min:             f64,
    max:             f64,
}

#[derive(Debug, Clone)]
struct BenchmarkSummary
{
    avg_frametime_ms:  Option<f64>,
    p99_frametime_ms:  Option<f64>,
    p999_frametime_ms: Option<f64>,
}

#[derive(Debug, Clone)]
struct DataFrameMeanMinMax
{
    avg_fps_by_group: HashMap<String, f64>,
    dist_rows:        Vec<DistStats>,
    fps_rows:         Vec<FpsOverTimeStats>,
}

#[derive(Debug, Clone, Default)]
struct ChartControlConfig
{
    min_max_trace_indices:   Vec<usize>,
    min_max_fill_colors:     Vec<String>,
    min_max_line_colors:     Vec<String>,
    fps_shape_indices:       Vec<usize>,
    fps_annotation_indices:  Vec<usize>,
    mean_fill_trace_indices: Vec<usize>,
    mean_fill_colors:        Vec<String>,
}

#[derive(Debug, Clone)]
struct GroupDirectoryData
{
    group_name:         String,
    ini_files_by_name:  BTreeMap<String, Vec<PathBuf>>,
    parsed_ini_by_name: BTreeMap<String, IniData>,
    notes_by_file:      BTreeMap<String, String>,
    summary:            BenchmarkSummary,
}

impl DataFrameMeanMinMax
{
    fn from_directory(directory: &Path, bench_run_folder_name: &String) -> Result<Self>
    {
        let grouped_runs =
            load_runs_from_nested_directories(directory, bench_run_folder_name, "run-")?;
        // let grouped_runs = load_runs_from_subdirectories(directory, "runs", "run-")?;
        let avg_fps_by_group = calculate_average_fps_by_group(&grouped_runs);
        let dist_rows = aggregate_distribution_by_bins(&grouped_runs, BIN_STEP);
        let fps_rows = aggregate_fps_over_time(&grouped_runs, TIME_BIN_PERCENT);

        Ok(Self {
            avg_fps_by_group,
            dist_rows,
            fps_rows,
        })
    }

    fn create_frametime_dist_plot(&self) -> Option<(Plot, ChartControlConfig)>
    {
        if self.dist_rows.is_empty()
        {
            return None;
        }

        let mut grouped: BTreeMap<String, Vec<&DistStats>> = BTreeMap::new();
        for row in &self.dist_rows
        {
            grouped
                .entry(row.file_group_name.clone())
                .or_default()
                .push(row);
        }

        let palette = palette_colors();
        let mut plot = Plot::new();
        let mut control = ChartControlConfig::default();
        let mut trace_index = 0usize;

        // Mimic Python behavior: an invisible anchor trace that provides unified hover title text.
        let mut x_hover: Vec<f64> = self.dist_rows.iter().map(|r| r.bin).collect();
        x_hover.sort_by(|a, b| a.total_cmp(b));
        x_hover.dedup_by(|a, b| a.total_cmp(b).is_eq());
        if x_hover.is_empty()
        {
            x_hover.push(0.0);
        }
        let y_hover = vec![0.0; x_hover.len()];
        let hover_title_text: Vec<String> = x_hover
            .iter()
            .map(|ms| {
                let fps = if *ms > 0.0 { 1000.0 / *ms } else { 0.0 };
                format!(
                    "<span style=\"font-size:{:.1}em;\">{:.1} ms | {:.1} fps</span>",
                    HOVER_TITLE_EM_SIZE, ms, fps
                )
            })
            .collect();

        let hover_anchor_trace = Scatter::new(x_hover, y_hover)
            .mode(Mode::Lines)
            .line(Line::new().color("rgba(0, 0, 0, 0)"))
            .text_array(hover_title_text)
            .hover_info(HoverInfo::Text)
            .hover_template("%{text}<extra></extra>")
            .show_legend(false);
        plot.add_trace(hover_anchor_trace);
        trace_index += 1;

        for (index, (group_name, mut rows)) in grouped.into_iter().enumerate()
        {
            rows.sort_by(|a, b| a.bin.total_cmp(&b.bin));

            let x: Vec<f64> = rows.iter().map(|r| r.bin).collect();
            let mut y_mean: Vec<f64> = rows.iter().map(|r| r.mean).collect();
            let mut y_min: Vec<f64> = rows.iter().map(|r| r.min).collect();
            let mut y_max: Vec<f64> = rows.iter().map(|r| r.max).collect();

            let total_mean: f64 = y_mean.iter().sum();
            if total_mean > 0.0
            {
                for value in &mut y_mean
                {
                    *value = (*value / total_mean) * 100.0;
                }
                for value in &mut y_min
                {
                    *value = (*value / total_mean) * 100.0;
                }
                for value in &mut y_max
                {
                    *value = (*value / total_mean) * 100.0;
                }
            }

            let color = palette[index % palette.len()];
            let min_max_fill = rgba(color, 0.18);
            let min_max_line = rgba(color, 0.35);
            let mean_fill = rgba(color, 0.22);

            let mut cumulative = 0.0;
            let hover_text: Vec<String> = y_mean
                .iter()
                .zip(y_min.iter())
                .zip(y_max.iter())
                .map(|((mean_val, min_val), max_val)| {
                    cumulative += *mean_val;
                    format!(
                        "Percentile = <b>{:.2}%</b><br><i>Mean={:.2}% | Min={:.2}% | Max={:.2}%</i>",
                        cumulative, mean_val, min_val, max_val
                    )
                })
                .collect();

            let mut x_area = x.clone();
            let mut y_area = y_max.clone();
            let mut x_rev = x.clone();
            x_rev.reverse();
            let mut y_rev = y_min.clone();
            y_rev.reverse();
            x_area.extend(x_rev);
            y_area.extend(y_rev);

            let min_max_trace = Scatter::new(x_area, y_area)
                .name(format!("{} Min/Max", group_name))
                .mode(Mode::Lines)
                .fill(Fill::ToSelf)
                .fill_color(min_max_fill.clone())
                .line(Line::new().color(min_max_line.clone()))
                .hover_info(HoverInfo::Skip)
                .show_legend(false)
                .legend_group(group_name.clone());
            plot.add_trace(min_max_trace);
            control.min_max_trace_indices.push(trace_index);
            control.min_max_fill_colors.push(min_max_fill);
            control.min_max_line_colors.push(min_max_line);
            trace_index += 1;

            let mean_trace = Scatter::new(x.clone(), y_mean.clone())
                .name(group_name.clone())
                .mode(Mode::Lines)
                .line(Line::new().color(color))
                .legend_group(group_name.clone())
                .text_array(hover_text)
                .hover_template("%{text}<extra></extra>");
            plot.add_trace(mean_trace);
            trace_index += 1;

            let mean_fill_trace = Scatter::new(x, y_mean)
                .name(format!("{} mean fill", group_name))
                .mode(Mode::Lines)
                .line(Line::new().color("rgba(0, 0, 0, 0)").width(0.0))
                .fill(Fill::ToZeroY)
                .fill_color("rgba(0, 0, 0, 0)")
                .hover_info(HoverInfo::Skip)
                .show_legend(false)
                .legend_group(group_name);
            plot.add_trace(mean_fill_trace);
            control.mean_fill_trace_indices.push(trace_index);
            control.mean_fill_colors.push(mean_fill);
            trace_index += 1;
        }

        let mut layout = Layout::new()
            .title("Frame Time Distribution")
            .template(&*PLOTLY_DARK)
            .hover_mode(HoverMode::XUnified)
            .show_legend(false)
            .x_axis(Axis::new().title("Frame Time (ms)").range(vec![0.0, {
                let data_max = self
                    .dist_rows
                    .iter()
                    .map(|r| r.bin)
                    .fold(f64::NEG_INFINITY, f64::max)
                    * 1.05;
                let hz_line_max = HZ_VALUES
                    .iter()
                    .map(|hz| 1000.0 / *hz as f64)
                    .fold(0.0, f64::max)
                    * 1.02;
                data_max.max(hz_line_max)
            }]))
            .y_axis(Axis::new().title("Percentage (%)"));

        for hz in HZ_VALUES
        {
            let frame_ms = 1000.0 / hz as f64;

            control
                .fps_shape_indices
                .push(control.fps_shape_indices.len());
            layout.add_shape(
                Shape::new()
                    .shape_type(ShapeType::Line)
                    .x_ref("x")
                    .y_ref("paper")
                    .x0(frame_ms)
                    .x1(frame_ms)
                    .y0(0.0)
                    .y1(1.0)
                    .visible(true)
                    .line(
                        ShapeLine::new()
                            .color("rgba(255, 80, 80, 0.5)")
                            .dash(DashType::Dash)
                            .width(1.0),
                    ),
            );

            control
                .fps_annotation_indices
                .push(control.fps_annotation_indices.len());
            layout.add_annotation(
                Annotation::new()
                    .x(frame_ms)
                    .x_ref("x")
                    .y(1.03)
                    .y_ref("paper")
                    .text(format!("{} Hz", hz))
                    .show_arrow(false)
                    .visible(true)
                    .font(Font::new().size(10).color("rgba(255, 130, 130, 0.85)")),
            );
        }

        plot.set_layout(layout);
        Some((plot, control))
    }

    fn create_framerate_over_time_plot(&self) -> Option<Plot>
    {
        if self.fps_rows.is_empty()
        {
            return None;
        }

        let mut grouped: BTreeMap<String, Vec<&FpsOverTimeStats>> = BTreeMap::new();
        for row in &self.fps_rows
        {
            grouped
                .entry(row.file_group_name.clone())
                .or_default()
                .push(row);
        }

        let palette = palette_colors();
        let mut plot = Plot::new();

        let mut anchor_x: Vec<f64> = self.fps_rows.iter().map(|row| row.run_pct_bin).collect();
        anchor_x.sort_by(|a, b| a.total_cmp(b));
        anchor_x.dedup_by(|a, b| a.total_cmp(b).is_eq());
        if anchor_x.is_empty()
        {
            anchor_x.push(0.0);
        }
        let hover_title_text: Vec<String> = anchor_x
            .iter()
            .map(|run_pct| {
                format!(
                    "<span style=\"font-size:{:.1}em;\">{:.1}% Run Time</span>",
                    HOVER_TITLE_EM_SIZE, run_pct
                )
            })
            .collect();

        let hover_anchor_trace = Scatter::new(anchor_x, vec![0.0; hover_title_text.len()])
            .mode(Mode::Lines)
            .line(Line::new().color("rgba(0, 0, 0, 0)"))
            .text_array(hover_title_text)
            .hover_info(HoverInfo::Text)
            .hover_template("%{text}<extra></extra>")
            .show_legend(false);
        plot.add_trace(hover_anchor_trace);

        for (index, (group_name, mut rows)) in grouped.into_iter().enumerate()
        {
            rows.sort_by(|a, b| a.run_pct_bin.total_cmp(&b.run_pct_bin));

            let x: Vec<f64> = rows.iter().map(|r| r.run_pct_bin).collect();
            let y_mean: Vec<f64> = rows.iter().map(|r| r.mean).collect();
            let y_min: Vec<f64> = rows.iter().map(|r| r.min).collect();
            let y_max: Vec<f64> = rows.iter().map(|r| r.max).collect();

            let color = palette[index % palette.len()];
            let avg_fps = self
                .avg_fps_by_group
                .get(&group_name)
                .copied()
                .unwrap_or_else(|| mean(&y_mean));
            let min_fps = y_min.iter().copied().reduce(f64::min).unwrap_or(0.0);
            let max_fps = y_max.iter().copied().reduce(f64::max).unwrap_or(0.0);
            let min_max_color = rgba(color, 0.35);

            let mut x_area = x.clone();
            let mut y_area = y_max.clone();
            let mut x_rev = x.clone();
            x_rev.reverse();
            let mut y_rev = y_min.clone();
            y_rev.reverse();
            x_area.extend(x_rev);
            y_area.extend(y_rev);

            let range_trace = Scatter::new(x_area, y_area)
                .name(format!("{} FPS range", group_name))
                .mode(Mode::Lines)
                .fill(Fill::ToSelf)
                .fill_color(rgba(color, 0.18))
                .line(Line::new().color(rgba(color, 0.32)))
                .hover_info(HoverInfo::Skip)
                .show_legend(false)
                .legend_group(group_name.clone());
            plot.add_trace(range_trace);

            let mean_trace = Scatter::new(x.clone(), y_mean.clone())
                .name(group_name.clone())
                .mode(Mode::Lines)
                .line(Line::new().color(color))
                .legend_group(group_name.clone())
                .text_array(
                    x.iter()
                        .zip(y_mean.iter())
                        .zip(y_min.iter().zip(y_max.iter()))
                        .map(|((_run_time, mean_fps), (min_fps, max_fps))| {
                            format!(
                                "Mean = {:.1} fps<br><i>Min={:.1} fps | Max={:.1} fps</i>",
                                mean_fps, min_fps, max_fps
                            )
                        })
                        .collect::<Vec<_>>(),
                )
                .hover_template("%{text}<extra></extra>");
            plot.add_trace(mean_trace);

            let avg_trace = Scatter::new(x.clone(), vec![avg_fps; x.len()])
                .name(format!("{} avg", group_name))
                .mode(Mode::Lines)
                .line(Line::new().color(color).dash(DashType::Dot).width(1.0))
                .legend_group(group_name.clone())
                .hover_info(HoverInfo::Skip)
                .show_legend(false);
            plot.add_trace(avg_trace);

            let line_label_x = (x.last().copied().unwrap_or(100.0) + 6.0).min(111.5);
            let avg_label_trace = Scatter::new(vec![line_label_x], vec![avg_fps])
                .name(format!("{} avg label", group_name))
                .mode(Mode::Text)
                .text_array(vec![format!("avg {:.1} fps", avg_fps)])
                .text_font(Font::new().color(color).size(9))
                .hover_info(HoverInfo::Skip)
                .show_legend(false)
                .legend_group(group_name.clone());
            plot.add_trace(avg_label_trace);

            let min_trace = Scatter::new(x.clone(), vec![min_fps; x.len()])
                .name(format!("{} min", group_name))
                .mode(Mode::Lines)
                .line(
                    Line::new()
                        .color(min_max_color.clone())
                        .dash(DashType::Dot)
                        .width(1.0),
                )
                .legend_group(group_name.clone())
                .hover_info(HoverInfo::Skip)
                .show_legend(false);
            plot.add_trace(min_trace);

            let min_label_trace = Scatter::new(vec![line_label_x], vec![min_fps])
                .name(format!("{} min label", group_name))
                .mode(Mode::Text)
                .text_array(vec![format!("min {:.1} fps", min_fps)])
                .text_font(Font::new().color(min_max_color.clone()).size(9))
                .hover_info(HoverInfo::Skip)
                .show_legend(false)
                .legend_group(group_name.clone());
            plot.add_trace(min_label_trace);

            let max_trace = Scatter::new(x.clone(), vec![max_fps; x.len()])
                .name(format!("{} max", group_name))
                .mode(Mode::Lines)
                .line(
                    Line::new()
                        .color(min_max_color.clone())
                        .dash(DashType::Dot)
                        .width(1.0),
                )
                .legend_group(group_name.clone())
                .hover_info(HoverInfo::Skip)
                .show_legend(false);
            plot.add_trace(max_trace);

            let max_label_trace = Scatter::new(vec![line_label_x], vec![max_fps])
                .name(format!("{} max label", group_name))
                .mode(Mode::Text)
                .text_array(vec![format!("max {:.1} fps", max_fps)])
                .text_font(Font::new().color(min_max_color).size(9))
                .hover_info(HoverInfo::Skip)
                .show_legend(false)
                .legend_group(group_name.clone());
            plot.add_trace(max_label_trace);
        }

        let layout = Layout::new()
            .title("Frame Rate Over Time")
            .template(&*PLOTLY_DARK)
            .hover_mode(HoverMode::XUnified)
            .show_legend(false)
            .x_axis(
                Axis::new()
                    .title("Benchmark Run Time (%)")
                    .range(vec![0.0, 112.0]),
            )
            .y_axis(
                Axis::new()
                    .title("Frame Rate (FPS)")
                    .range_mode(RangeMode::ToZero),
            );

        plot.set_layout(layout);
        Some(plot)
    }
}

pub fn generate_chart(directory: &Path, bench_run_folder_name: &String) -> Result<PathBuf>
{
    let combined = DataFrameMeanMinMax::from_directory(directory, bench_run_folder_name)?;
    let mut group_directory_data = collect_group_directory_data(directory)?;

    // Ensure group summaries include frametime data found in nested run CSVs.
    // `collect_single_group_directory_data` only reads files directly under the group folder,
    // but runs may be nested. Merge recursive run CSVs into each group's summary here.
    if let Ok(grouped_runs) =
        load_runs_from_nested_directories(directory, bench_run_folder_name, "run-")
    {
        for group in &mut group_directory_data
        {
            if let Some(runs) = grouped_runs.get(&group.group_name)
            {
                let mut all: Vec<f64> = Vec::new();
                for run in runs
                {
                    all.extend_from_slice(&run.frametimes_ms);
                }
                group.summary = benchmark_summary_from_frametimes(all);
            }
        }
    }

    // Try to build both plots — if one is missing, render a placeholder and continue
    // so the comparison table and page still render instead of returning an error.
    let (frame_time_plot_html, control_config) = match combined.create_frametime_dist_plot()
    {
        Some((plot, cfg)) => (
            inject_chart_hover_title_hiding(
                &plot.to_inline_html(Some(FRAME_TIME_PLOT_ID)),
                FRAME_TIME_PLOT_ID,
                false,
            ),
            cfg,
        ),
        None => (
            "<div class=\"no-data\">No frame-time distribution data available.</div>".to_string(),
            ChartControlConfig::default(),
        ),
    };

    let frame_rate_plot_html = match combined.create_framerate_over_time_plot()
    {
        Some(plot) => inject_chart_hover_title_hiding(
            &plot.to_inline_html(Some(FRAME_RATE_PLOT_ID)),
            FRAME_RATE_PLOT_ID,
            true,
        ),
        None => "<div class=\"no-data\">No frame-rate over time data available.</div>".to_string(),
    };

    let color_map = build_group_color_map(
        combined
            .dist_rows
            .iter()
            .map(|row| row.file_group_name.clone())
            .chain(
                group_directory_data
                    .iter()
                    .map(|group| group.group_name.clone()),
            )
            .collect(),
    );

    let linked_legend_html = build_ini_link_legend_html(&group_directory_data, &color_map)?;
    let comparison_table_html = build_ini_comparison_table_html(&group_directory_data, &color_map)?;
    let inline_css_html = build_inline_css_tag();
    let inline_js_html = build_inline_js_tag(&control_config);

    let page_html = build_combined_analysis_page(
        &frame_time_plot_html,
        &frame_rate_plot_html,
        &linked_legend_html,
        &comparison_table_html,
        &inline_css_html,
        &inline_js_html,
    );

    let output_file = build_results_output_path(directory)?;
    fs::write(&output_file, page_html)
        .with_context(|| format!("failed writing HTML output: {}", output_file.display()))?;

    Ok(output_file)
}

fn get_parent_from_path<'a>(path: &'a Path, blacklist: Vec<&'a str>) -> Result<&'a Path>
{
    if let Some(parent) = path.parent()
    {
        if !blacklist.contains(
            &parent
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("unknown"),
        )
        {
            Ok(parent)
        }
        else
        {
            get_parent_from_path(parent, blacklist)
        }
    }
    else
    {
        Err(anyhow!("no parent directory found"))
    }
}

fn load_runs_from_nested_directories(
    directory: &Path,
    bench_run_folder_name: &String,
    groupby_before_delim: &str,
) -> Result<HashMap<String, Vec<RunData>>>
{
    let mut result: HashMap<String, Vec<RunData>> = HashMap::new();

    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed reading directory: {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir()
        {
            let sub_runs = load_runs_from_nested_directories(
                &path,
                bench_run_folder_name,
                groupby_before_delim,
            )
            .with_context(|| {
                format!("failed loading runs from subdirectory: {}", path.display())
            })?;

            // Merge runs found in the subdirectory into the current result.
            // Previously this only merged when the folder matched `bench_run_folder_name`,
            // which discarded runs when CSVs lived under group folders directly.
            for (group_name, runs) in sub_runs
            {
                result.entry(group_name).or_default().extend(runs);
            }
        }
        else if path.is_file()
        {
            let file_name = path
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or_default()
                .to_string();

            if !file_name.ends_with(".csv") || !file_name.contains(groupby_before_delim)
            {
                continue;
            }

            let group_name = get_parent_from_path(&path, vec![bench_run_folder_name])?
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("unknown")
                .to_string();

            // println!("Group name: {}, File: {}", group_name, file_name);

            let frametimes_ms = load_frametimes_from_csv(&path)
                .with_context(|| format!("failed loading CSV: {}", path.display()))?;

            if frametimes_ms.is_empty()
            {
                continue;
            }

            result.entry(group_name).or_default().push(RunData {
                #[cfg(false)]
                file_name,
                frametimes_ms,
            })
        }
    }

    Ok(result)
}

#[cfg(false)]
fn load_runs_from_subdirectories(
    directory: &Path,
    runs_dir_name: &str,
    groupby_before_delim: &str,
) -> Result<HashMap<String, Vec<RunData>>>
{
    let mut result: HashMap<String, Vec<RunData>> = HashMap::new();

    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed reading directory: {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir()
        {
            continue;
        }

        let group_name = path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("unknown")
            .to_string();

        for file in fs::read_dir(&path)
            .with_context(|| format!("failed reading subdirectory: {}", path.display()))?
        {
            let file = file?;
            let file_path = file.path();
            if !file_path.is_file()
            {
                continue;
            }

            let file_name = file_path
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or_default()
                .to_string();

            if !file_name.ends_with(".csv") || !file_name.contains(groupby_before_delim)
            {
                continue;
            }

            let frametimes_ms = load_frametimes_from_csv(&file_path)
                .with_context(|| format!("failed loading CSV: {}", file_path.display()))?;

            if frametimes_ms.is_empty()
            {
                continue;
            }

            result
                .entry(group_name.clone())
                .or_default()
                .push(RunData { frametimes_ms });
        }
    }

    Ok(result)
}

fn collect_group_directory_data(directory: &Path) -> Result<Vec<GroupDirectoryData>>
{
    let mut group_paths = Vec::new();

    for entry in fs::read_dir(directory)
        .with_context(|| format!("failed reading directory: {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir()
        {
            continue;
        }

        let Some(folder_name) = path.file_name().and_then(OsStr::to_str)
        else
        {
            continue;
        };

        if folder_name.starts_with('.')
        {
            continue;
        }

        let group_name = folder_name.to_string();
        group_paths.push((group_name, path));
    }

    let mut results: Vec<GroupDirectoryData> = group_paths
        .into_par_iter()
        .map(|(group_name, group_path)| collect_single_group_directory_data(group_name, group_path))
        .collect::<Result<Vec<_>>>()?;

    results.sort_by(|left, right| left.group_name.cmp(&right.group_name));

    Ok(results)
}

fn collect_single_group_directory_data(
    group_name: String,
    group_path: PathBuf,
) -> Result<GroupDirectoryData>
{
    let mut ini_files_by_name: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    let mut parsed_ini_by_name: BTreeMap<String, IniData> = BTreeMap::new();
    let mut notes_by_file: BTreeMap<String, String> = BTreeMap::new();
    let mut frametimes = Vec::new();

    for file in fs::read_dir(&group_path)
        .with_context(|| format!("failed reading subdirectory: {}", group_path.display()))?
    {
        let file = file?;
        let file_path = file.path();
        if !file_path.is_file()
        {
            continue;
        }

        let file_name = file_path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
            .to_string();

        if file_name.ends_with(".ini")
        {
            let ini_file_name = file_name.clone();
            if let Ok(parsed_ini) = parse_ini_key_value_lines(&file_path)
            {
                parsed_ini_by_name.insert(file_name, parsed_ini);
            }
            ini_files_by_name
                .entry(ini_file_name)
                .or_default()
                .push(file_path);
            continue;
        }

        if file_name.to_ascii_lowercase().contains(".ir_bench.")
        {
            if let Ok(content) = fs::read_to_string(&file_path)
            {
                let trimmed = content.trim();
                if !trimmed.is_empty()
                {
                    notes_by_file.insert(file_name, trimmed.to_string());
                }
            }
            continue;
        }

        if file_name.ends_with(".csv")
            && file_name.contains("run-")
            && let Ok(values) = load_frametimes_from_csv(&file_path)
        {
            frametimes.extend(values);
        }
    }

    for paths in ini_files_by_name.values_mut()
    {
        paths.sort();
    }

    Ok(GroupDirectoryData {
        group_name,
        ini_files_by_name,
        parsed_ini_by_name,
        notes_by_file,
        summary: benchmark_summary_from_frametimes(frametimes),
    })
}

fn load_frametimes_from_csv(csv_file_path: &Path) -> Result<Vec<f64>>
{
    let mut reader = csv::Reader::from_path(csv_file_path)
        .with_context(|| format!("failed to open CSV file: {}", csv_file_path.display()))?;

    let headers = reader
        .headers()
        .with_context(|| format!("failed reading CSV headers: {}", csv_file_path.display()))?
        .clone();

    let idx = headers
        .iter()
        .position(|h| h == "MsBetweenPresents")
        .context("MsBetweenPresents column not found")?;

    let mut frametimes = Vec::new();
    for row in reader.records()
    {
        let record: StringRecord = row?;
        if let Some(cell) = record.get(idx)
            && let Ok(value) = cell.trim().parse::<f64>()
            && value.is_finite()
            && value > 0.0
        {
            frametimes.push(value);
        }
    }

    Ok(frametimes)
}

fn benchmark_summary_from_frametimes(mut frametimes: Vec<f64>) -> BenchmarkSummary
{
    if frametimes.is_empty()
    {
        return BenchmarkSummary {
            avg_frametime_ms:  None,
            p99_frametime_ms:  None,
            p999_frametime_ms: None,
        };
    }

    frametimes.sort_by(|a, b| a.total_cmp(b));
    BenchmarkSummary {
        avg_frametime_ms:  Some(mean(&frametimes)),
        p99_frametime_ms:  percentile_sorted(&frametimes, 0.99),
        p999_frametime_ms: percentile_sorted(&frametimes, 0.999),
    }
}

fn aggregate_distribution_by_bins(
    grouped_runs: &HashMap<String, Vec<RunData>>,
    bin_step: f64,
) -> Vec<DistStats>
{
    let mut grouped_counts: BTreeMap<(String, i64), Vec<f64>> = BTreeMap::new();

    for (group_name, runs) in grouped_runs
    {
        for run in runs
        {
            let mut counts_per_bin: BTreeMap<i64, usize> = BTreeMap::new();
            for &frametime in &run.frametimes_ms
            {
                let bin_key = round_to_scaled_bin(frametime, bin_step);
                *counts_per_bin.entry(bin_key).or_insert(0) += 1;
            }

            for (bin_key, count) in counts_per_bin
            {
                grouped_counts
                    .entry((group_name.clone(), bin_key))
                    .or_default()
                    .push(count as f64);
            }
        }
    }

    grouped_counts
        .into_iter()
        .map(|((group_name, bin_key), values)| DistStats {
            file_group_name: group_name,
            bin:             scaled_bin_to_f64(bin_key, bin_step),
            mean:            mean(&values),
            min:             values.iter().copied().fold(f64::INFINITY, f64::min),
            max:             values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        })
        .collect()
}

fn aggregate_fps_over_time(
    grouped_runs: &HashMap<String, Vec<RunData>>,
    time_bin_percent: f64,
) -> Vec<FpsOverTimeStats>
{
    let mut grouped_fps: BTreeMap<(String, i64), Vec<f64>> = BTreeMap::new();

    for (group_name, runs) in grouped_runs
    {
        for run in runs
        {
            let total_ms: f64 = run.frametimes_ms.iter().sum();
            if total_ms <= 0.0
            {
                continue;
            }

            let mut elapsed_ms = 0.0;
            let mut fps_by_bin: BTreeMap<i64, Vec<f64>> = BTreeMap::new();
            for frametime_ms in &run.frametimes_ms
            {
                elapsed_ms += frametime_ms;
                let run_pct = (elapsed_ms / total_ms) * 100.0;
                let pct_bin_key = round_to_scaled_bin(run_pct, time_bin_percent);
                fps_by_bin
                    .entry(pct_bin_key)
                    .or_default()
                    .push(1000.0 / frametime_ms);
            }

            for (pct_bin_key, fps_values) in fps_by_bin
            {
                grouped_fps
                    .entry((group_name.clone(), pct_bin_key))
                    .or_default()
                    .push(mean(&fps_values));
            }
        }
    }

    grouped_fps
        .into_iter()
        .map(|((group_name, pct_bin_key), values)| FpsOverTimeStats {
            file_group_name: group_name,
            run_pct_bin:     scaled_bin_to_f64(pct_bin_key, time_bin_percent),
            mean:            mean(&values),
            min:             values.iter().copied().fold(f64::INFINITY, f64::min),
            max:             values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        })
        .collect()
}

fn calculate_average_fps_by_group(
    grouped_runs: &HashMap<String, Vec<RunData>>,
) -> HashMap<String, f64>
{
    let mut result = HashMap::new();

    for (group_name, runs) in grouped_runs
    {
        let mut all = Vec::new();
        for run in runs
        {
            all.extend(run.frametimes_ms.iter().copied());
        }

        let avg_frametime_ms = mean(&all);
        if avg_frametime_ms > 0.0
        {
            result.insert(group_name.clone(), 1000.0 / avg_frametime_ms);
        }
    }

    result
}

#[derive(Debug, Clone)]
struct IniData
{
    #[cfg(false)]
    keys_in_order:   Vec<String>,
    values_by_key:   HashMap<String, String>,
    comments_by_key: HashMap<String, String>,
}

fn parse_ini_key_value_lines(path: &Path) -> Result<IniData>
{
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read INI file: {}", path.display()))?;

    #[cfg(false)]
    let mut keys_in_order = Vec::new();
    let mut values_by_key = HashMap::new();
    let mut comments_by_key = HashMap::new();

    for line in content.lines()
    {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('[')
        {
            continue;
        }

        let Some((key_raw, remainder)) = line.split_once('=')
        else
        {
            continue;
        };

        let key = key_raw.trim();
        if key.is_empty()
        {
            continue;
        }

        let (value, comment) = if let Some((value_part, comment_part)) = remainder.split_once(';')
        {
            (
                value_part.trim().to_string(),
                comment_part.trim().to_string(),
            )
        }
        else
        {
            (remainder.trim().to_string(), String::new())
        };

        #[cfg(false)]
        if !values_by_key.contains_key(key)
        {
            keys_in_order.push(key.to_string());
        }
        values_by_key.insert(key.to_string(), value);
        comments_by_key.insert(key.to_string(), comment);
    }

    Ok(IniData {
        #[cfg(false)]
        keys_in_order,
        values_by_key,
        comments_by_key,
    })
}

#[cfg(false)]
fn get_ini_changed_keys(base: &IniData, compare: &IniData) -> BTreeSet<String>
{
    let mut keys = BTreeSet::new();

    for key in base
        .values_by_key
        .keys()
        .chain(compare.values_by_key.keys())
    {
        let a = base.values_by_key.get(key);
        let b = compare.values_by_key.get(key);
        if a != b
        {
            keys.insert(key.clone());
        }
    }

    keys
}

fn get_ini_all_keys(data_by_group: &HashMap<String, &IniData>) -> BTreeSet<String>
{
    let mut keys = BTreeSet::new();

    for data in data_by_group.values()
    {
        keys.extend(data.values_by_key.keys().cloned());
    }

    keys
}

fn build_ini_comparison_table_html(
    group_data: &[GroupDirectoryData],
    color_map: &HashMap<String, String>,
) -> Result<String>
{
    let all_groups = group_data
        .iter()
        .map(|group| group.group_name.clone())
        .collect::<Vec<_>>();
    let mut grouped_ini_files: BTreeMap<String, Vec<(String, &IniData)>> = BTreeMap::new();

    for group in group_data
    {
        for (file_name, parsed_ini) in &group.parsed_ini_by_name
        {
            grouped_ini_files
                .entry(file_name.clone())
                .or_default()
                .push((group.group_name.clone(), parsed_ini));
        }
    }
    if grouped_ini_files.is_empty()
    {
        return Ok(
            "<div class=\"no-differences\">No INI file differences found.</div>".to_string(),
        );
    }
    let mut html = String::new();

    html.push_str("<table class=\"ini-comparison-table\"><thead><tr><th>Key</th><th>Comment</th>");
    for group_name in &all_groups
    {
        let color = color_map
            .get(group_name)
            .cloned()
            .unwrap_or_else(|| "#f2f2f2".to_string());
        let (datetime_part, rest_part) = split_group_name_parts(group_name);
        html.push_str(&format!(
            "<th class=\"group-column-header\" data-group=\"{}\"><span class=\"diff-header-name\"><span class=\"diff-header-name-rest\" style=\"color:{};\">{}</span><span class=\"diff-header-name-datetime\">{}</span></span></th>",
            escape_html(group_name),
            color,
            escape_html(&rest_part),
            escape_html(&datetime_part)
        ));
    }
    html.push_str("</tr></thead><tbody>");

    html.push_str(&format!(
        "<tr class=\"table-section-row table-major-section-row\"><td colspan=\"{}\">Benchmark Notes</td></tr>",
        2 + all_groups.len()
    ));

    html.push_str(&build_notes_rows_html(group_data, &all_groups)?);

    html.push_str(&format!(
        "<tr class=\"table-section-row table-major-section-row\"><td colspan=\"{}\">Performance Summary</td></tr>",
        2 + all_groups.len()
    ));

    html.push_str(&build_summary_rows_html(group_data, &all_groups));

    html.push_str(&format!(
        "<tr class=\"table-section-row table-major-section-row\"><td colspan=\"{}\">INI Differences</td></tr>",
        2 + all_groups.len()
    ));

    let mut has_diff_rows = false;

    for (ini_name, ini_paths) in grouped_ini_files
    {
        let data_by_group: HashMap<String, &IniData> = ini_paths
            .iter()
            .map(|(group_name, parsed_ini)| (group_name.clone(), *parsed_ini))
            .collect();

        let baseline_group = match all_groups
            .iter()
            .find(|group_name| data_by_group.contains_key(*group_name))
        {
            Some(name) => name,
            None => continue,
        };
        let Some(base_data) = data_by_group.get(baseline_group)
        else
        {
            continue;
        };

        let all_keys = get_ini_all_keys(&data_by_group);

        html.push_str(&format!(
            "<tr class=\"table-section-row table-subsection-row\"><td colspan=\"{}\"><span class=\"ini-section-header\"><span class=\"ini-section-name\">{}</span><span class=\"ini-section-count\" data-visible-row-count>0 rows</span></span></td></tr>",
            2 + all_groups.len(),
            escape_html(&ini_name)
        ));

        for key in all_keys
        {
            has_diff_rows = true;
            let comment = base_data
                .comments_by_key
                .get(&key)
                .cloned()
                .unwrap_or_default();
            html.push_str("<tr class=\"changed-row\">");
            html.push_str(&format!(
                "<td class=\"key-cell\">{}</td>",
                escape_html(&key)
            ));
            html.push_str(&format!(
                "<td class=\"comment-cell\">{}</td>",
                escape_html_with_line_breaks(&comment)
            ));

            let baseline_value = base_data
                .values_by_key
                .get(&key)
                .cloned()
                .unwrap_or_default();
            let baseline_number = baseline_value.parse::<f64>().ok();

            for group in &all_groups
            {
                let value = data_by_group
                    .get(group)
                    .and_then(|data| data.values_by_key.get(&key))
                    .cloned()
                    .unwrap_or_default();
                let has_value = !value.trim().is_empty();

                let (color, diff_state, diff_direction, background_color) = if group
                    == baseline_group
                {
                    (
                        "#9ccfff",
                        "baseline",
                        "baseline",
                        "linear-gradient(90deg, rgba(107, 163, 208, 0.24), rgba(107, 163, 208, 0.07))",
                    )
                }
                else
                {
                    let color = choose_diff_color(&baseline_value, &value, baseline_number);
                    let (text_color, diff_direction, background_color) = match color
                    {
                        "#7cc576" => (
                            "#a5f09d",
                            "better",
                            "linear-gradient(90deg, rgba(124, 197, 118, 0.32), rgba(124, 197, 118, 0.09))",
                        ),
                        "#ff6b6b" => (
                            "#ff9f9f",
                            "worse",
                            "linear-gradient(90deg, rgba(255, 107, 107, 0.32), rgba(255, 107, 107, 0.09))",
                        ),
                        _ => (
                            "#8ec7ff",
                            "equal",
                            "linear-gradient(90deg, rgba(107, 163, 208, 0.22), rgba(107, 163, 208, 0.06))",
                        ),
                    };
                    let diff_state = if color == "#6ba3d0"
                    {
                        "equal"
                    }
                    else
                    {
                        "changed"
                    };
                    (text_color, diff_state, diff_direction, background_color)
                };

                html.push_str(&format!(
                    "<td class=\"value-cell\" data-group=\"{}\" data-diff-state=\"{}\" data-diff-direction=\"{}\" data-has-value=\"{}\" data-raw-value=\"{}\" style=\"color:{}; background:{};\">{}</td>",
                    escape_html(group),
                    diff_state,
                    diff_direction,
                    has_value,
                    escape_html(&value),
                    color,
                    background_color,
                    escape_html_with_line_breaks(&value)
                ));
            }

            html.push_str("</tr>");
        }
    }

    if !has_diff_rows
    {
        html.push_str(&format!(
            "<tr><td class=\"notes-row-cell\" colspan=\"{}\"><span class=\"notes-empty\">No INI file differences found.</span></td></tr>",
            2 + all_groups.len()
        ));
    }

    html.push_str("</tbody></table>");
    Ok(format!(
        "<div class=\"ini-comparison-tables\">{}</div>",
        html
    ))
}

fn build_notes_rows_html(group_data: &[GroupDirectoryData], all_groups: &[String])
-> Result<String>
{
    let mut all_note_names: BTreeSet<String> = BTreeSet::new();
    for group in group_data
    {
        all_note_names.extend(group.notes_by_file.keys().cloned());
    }

    if all_note_names.is_empty()
    {
        return Ok(format!(
            "<tr><td class=\"notes-row-cell\" colspan=\"{}\"><span class=\"notes-empty\">No *.ir_bench.* files found.</span></td></tr>",
            2 + all_groups.len()
        ));
    }

    let mut html = String::new();

    for note_name in all_note_names
    {
        html.push_str("<tr class=\"notes-collapsible-row\">");
        html.push_str(&format!(
            "<td class=\"key-cell notes-file-key-cell\">{}</td>",
            escape_html(&note_name)
        ));

        let kind = if note_name.to_ascii_lowercase().ends_with(".md")
        {
            "Markdown"
        }
        else
        {
            "Text"
        };

        html.push_str(&format!(
            "<td class=\"comment-cell notes-file-comment-cell\"><span class=\"notes-entry-kind\">{}</span></td>",
            kind
        ));

        for group in all_groups
        {
            let value = group_data
                .iter()
                .find(|entry| &entry.group_name == group)
                .and_then(|entry| entry.notes_by_file.get(&note_name))
                .cloned();

            match value
            {
                Some(text) =>
                {
                    let preview = build_note_preview(&text);
                    html.push_str(&format!(
                        "<td class=\"value-cell notes-row-cell\" data-group=\"{}\"><details class=\"notes-file-details\"><summary class=\"notes-file-summary\"><span>View</span><span class=\"notes-row-preview\">{}</span></summary><pre class=\"notes-body\">{}</pre></details></td>",
                        escape_html(group),
                        escape_html(&preview),
                        escape_html(&text)
                    ));
                }
                None =>
                {
                    html.push_str(&format!(
                        "<td class=\"value-cell notes-row-cell\" data-group=\"{}\"><span class=\"notes-empty\">-</span></td>",
                        escape_html(group)
                    ));
                }
            }
        }

        html.push_str("</tr>");
    }

    Ok(html)
}

fn build_summary_rows_html(group_data: &[GroupDirectoryData], all_groups: &[String]) -> String
{
    let summaries: HashMap<String, BenchmarkSummary> = group_data
        .iter()
        .map(|group| (group.group_name.clone(), group.summary.clone()))
        .collect();

    let rows = [
        (
            "Average",
            "Frametime: Lower is better\nFPS: Higher is better",
            "avg",
        ),
        (
            "1% Low",
            "99th percentile frametime\nFPS: Higher is better",
            "p99",
        ),
        (
            "0.1% Low",
            "99.9th percentile frametime\nFPS: Higher is better",
            "p999",
        ),
    ];

    let mut html = String::new();

    // Prefer a baseline group that actually has summary metrics available.
    let baseline_group = all_groups
        .iter()
        .find(|g| {
            if let Some(s) = summaries.get(*g)
            {
                s.avg_frametime_ms.is_some()
                    || s.p99_frametime_ms.is_some()
                    || s.p999_frametime_ms.is_some()
            }
            else
            {
                false
            }
        })
        .cloned()
        .or_else(|| all_groups.first().cloned())
        .unwrap_or_default();

    for (metric_name, description, metric_key) in rows
    {
        let metric_value_for = |s: &BenchmarkSummary| -> Option<f64> {
            match metric_key
            {
                "avg" => s.avg_frametime_ms,
                "p99" => s.p99_frametime_ms,
                "p999" => s.p999_frametime_ms,
                _ => None,
            }
        };

        let baseline_ms = summaries
            .get(&baseline_group)
            .and_then(metric_value_for)
            .unwrap_or(f64::NAN);

        html.push_str("<tr>");
        html.push_str(&format!("<td class=\"key-cell\">{}</td>", metric_name));
        html.push_str(&format!(
            "<td class=\"comment-cell summary-comment-cell\">{}</td>",
            escape_html_with_line_breaks(description)
        ));

        for group in all_groups
        {
            let current = summaries.get(group).and_then(metric_value_for);
            let frametime_attr = current
                .map(|value| format!(" data-frametime-ms=\"{value:.8}\""))
                .unwrap_or_default();
            let (summary_state, summary_background_color) = summary_state_and_background(
                current,
                if baseline_ms.is_finite()
                {
                    Some(baseline_ms)
                }
                else
                {
                    None
                },
                group == &baseline_group,
            );
            html.push_str(&format!(
                "<td class=\"value-cell stat-value\" data-group=\"{}\" data-metric-key=\"{}\" data-summary-state=\"{}\"{} style=\"background:{};\">{}</td>",
                escape_html(group),
                metric_key,
                summary_state,
                frametime_attr,
                summary_background_color,
                format_metric_with_indicator(
                    current,
                    if baseline_ms.is_finite()
                    {
                        Some(baseline_ms)
                    }
                    else
                    {
                        None
                    },
                    group != &baseline_group
                )
            ));
        }

        html.push_str("</tr>");
    }

    html
}

fn build_ini_link_legend_html(
    group_data: &[GroupDirectoryData],
    color_map: &HashMap<String, String>,
) -> Result<String>
{
    let mut sections = Vec::new();

    for group in group_data
    {
        let ini_files = group
            .ini_files_by_name
            .values()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();

        if ini_files.len() < 2
        {
            continue;
        }

        let color = color_map
            .get(&group.group_name)
            .cloned()
            .unwrap_or_else(|| "#999999".to_string());

        let links = ini_files
            .iter()
            .take(2)
            .map(|path| {
                let name = path
                    .file_name()
                    .and_then(OsStr::to_str)
                    .unwrap_or("ini");
                format!(
                    "<a href=\"file:///{path}\" target=\"_blank\" rel=\"noopener noreferrer\">{name}</a>",
                    path = path.display().to_string().replace("\\", "/"),
                    name = escape_html(name)
                )
            })
            .collect::<Vec<_>>()
            .join(" ");

        let escaped_group = escape_html(&group.group_name);
        let (datetime_part, rest_part) = split_group_name_parts(&group.group_name);

        sections.push(format!(
            "<div class=\"legend-row\" data-group=\"{group}\" draggable=\"true\"><input type=\"checkbox\" class=\"group-visibility-checkbox group-pane-checkbox\" data-group=\"{group}\" checked style=\"accent-color:{color};\"><div class=\"legend-content\"><div class=\"legend-name-row\"><span class=\"legend-name\"><span class=\"legend-name-rest\">{rest}</span><span class=\"legend-name-datetime\">{datetime}</span></span></div><span class=\"legend-links\">{links}</span></div></div>",
            group = escaped_group,
            rest = escape_html(&rest_part),
            datetime = escape_html(&datetime_part),
            color = color,
            links = links
        ));
    }

    if sections.is_empty()
    {
        return Ok(
            "<div class=\"no-differences\">No INI files found for linked legend entries.</div>"
                .to_string(),
        );
    }

    let controls_html = "<div class=\"linked-legend-controls\"><button type=\"button\" id=\"linked-legend-check-all\" class=\"legend-bulk-button\">check all</button><button type=\"button\" id=\"linked-legend-uncheck-all\" class=\"legend-bulk-button\">uncheck all</button></div>";
    Ok(format!(
        "{}<div class='linked-legend'>{}</div>",
        controls_html,
        sections.join("")
    ))
}

fn build_combined_analysis_page(
    frame_time_chart_html: &str,
    frame_rate_chart_html: &str,
    linked_legend_html: &str,
    comparison_table_html: &str,
    inline_css_html: &str,
    inline_js_html: &str,
) -> String
{
    let diff_table_panel = if comparison_table_html.is_empty()
    {
        String::new()
    }
    else
    {
        format!(
            "<div id=\"diff-table-panel\" class=\"panel\" tabindex=\"-1\"><h2>Comparison Table</h2>{}</div>",
            comparison_table_html
        )
    };

    let template = include_str!("assets/template.html")
        .replace("\\\"", "\"")
        .replace("{{", "{")
        .replace("}}", "}");

    template
        .replace("{__INLINE_HTML_FRAME_TIME_CHART__}", frame_time_chart_html)
        .replace("{__INLINE_HTML_FRAME_RATE_CHART__}", frame_rate_chart_html)
        .replace("{__INLINE_HTML_LINKED_LEGEND__}", linked_legend_html)
        .replace("{__INLINE_HTML_DIFF_TABLE_PANEL__}", &diff_table_panel)
        .replace("{__INLINE_CSS__}", inline_css_html)
        .replace("{__INLINE_JS__}", inline_js_html)
}

fn build_inline_css_tag() -> String
{
    // Inline styles to keep the generated report fully self-contained.
    format!("<style>\n{}\n</style>", include_str!("assets/template.css"))
}

fn build_inline_js_tag(config: &ChartControlConfig) -> String
{
    // Prevent a literal closing script tag in content from ending this inline script early.
    let js_content = include_str!("assets/template.js")
        .replace(
            "__CONTROL_CONFIG_JSON__",
            &build_control_config_json(config),
        )
        .replace("</script>", "<\\/script>");
    format!("<script>\n{}\n</script>", js_content)
}

fn build_control_config_json(config: &ChartControlConfig) -> String
{
    json!({
        "min_max_trace_indices": &config.min_max_trace_indices,
        "min_max_fill_colors": &config.min_max_fill_colors,
        "min_max_line_colors": &config.min_max_line_colors,
        "fps_shape_indices": &config.fps_shape_indices,
        "fps_annotation_indices": &config.fps_annotation_indices,
        "mean_fill_trace_indices": &config.mean_fill_trace_indices,
        "mean_fill_colors": &config.mean_fill_colors
    })
    .to_string()
}

fn inject_hidden_unified_hover_title(chart_html: &str) -> String
{
    if chart_html.contains("\"unifiedhovertitle\":")
    {
        return chart_html.to_string();
    }

    let target = "\"hovermode\":\"x unified\",\"xaxis\":{";
    let replacement = format!(
        "\"hovermode\":\"x unified\",\"xaxis\":{{\"unifiedhovertitle\":{{\"text\":\"{}\"}},",
        HIDDEN_UNIFIED_X_TITLE_HTML
    );

    chart_html.replacen(target, &replacement, 1)
}

fn inject_chart_hover_title_hiding(
    chart_html: &str,
    plot_id: &str,
    keep_first_title_only: bool,
) -> String
{
    let with_layout_injection = inject_hidden_unified_hover_title(chart_html);

    // Fallback for Plotly builds that ignore xaxis.unifiedhovertitle:
    // hide the unified hover x-title node in the rendered hover layer.
    if with_layout_injection.contains("__IR_BENCH_HIDE_X_UNIFIED_TITLE__")
    {
        return with_layout_injection;
    }

    let fallback_style = format!(
        "<style>#{plot_id} .hoverlayer text.legendtitletext {{ display: none !important; }}</style>",
        plot_id = plot_id
    );

    let fallback_script = format!(
        r#"<script>
;(function() {{
  var gd = document.getElementById('{plot_id}');
  if (!gd) return;

  function hideUnifiedXTitle() {{
    var root = gd.querySelector('.hoverlayer');
    if (!root) return;

        var nodes = root.querySelectorAll('text.legendtitletext');
        nodes.forEach(function(node, index) {{
            var shouldShow = {keep_first_title_only_js} && index === 0;
            node.style.display = shouldShow ? '' : 'none';
            node.setAttribute('data-ir-bench', shouldShow ? '__IR_BENCH_KEEP_X_UNIFIED_TITLE__' : '__IR_BENCH_HIDE_X_UNIFIED_TITLE__');
        }});
  }}

  gd.on && gd.on('plotly_hover', hideUnifiedXTitle);
  gd.on && gd.on('plotly_relayout', hideUnifiedXTitle);
  gd.on && gd.on('plotly_afterplot', hideUnifiedXTitle);

  hideUnifiedXTitle();
}})();
</script>"#,
        plot_id = plot_id,
        keep_first_title_only_js = if keep_first_title_only
        {
            "true"
        }
        else
        {
            "false"
        }
    );

    format!(
        "{}{}{}",
        with_layout_injection, fallback_style, fallback_script
    )
}

fn split_group_name_parts(group_name: &str) -> (String, String)
{
    let mut parts = group_name.splitn(2, ' ');
    let datetime_part = parts.next().unwrap_or_default().to_string();
    let rest_part = parts.next().unwrap_or_default().to_string();
    (datetime_part, rest_part)
}

fn build_group_color_map(group_names: Vec<String>) -> HashMap<String, String>
{
    let palette = palette_colors();
    let mut unique = BTreeSet::new();
    unique.extend(group_names);

    unique
        .into_iter()
        .enumerate()
        .map(|(index, name)| (name, palette[index % palette.len()].to_string()))
        .collect()
}

fn build_results_output_path(directory: &Path) -> Result<PathBuf>
{
    let mut subfolders = fs::read_dir(directory)
        .with_context(|| format!("failed reading directory: {}", directory.display()))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            get_subfolder_datetime_part(&name).map(|_| name)
        })
        .collect::<Vec<_>>();

    subfolders.sort();

    let mut prefix = String::new();
    if let (Some(first), Some(last)) = (subfolders.first(), subfolders.last())
        && let (Some(earliest), Some(latest)) = (
            sanitize_filename_part(get_subfolder_datetime_part(first).as_deref()),
            sanitize_filename_part(get_subfolder_datetime_part(last).as_deref()),
        )
    {
        prefix = format!("{}_{}_", latest, earliest);
    }

    Ok(directory.join(format!("{}results.ir_bench.rs.html", prefix)))
}

fn get_subfolder_datetime_part(subfolder_name: &str) -> Option<String>
{
    subfolder_name
        .split_once(' ')
        .map(|(left, _)| left.trim().to_string())
}

fn sanitize_filename_part(value: Option<&str>) -> Option<String>
{
    value.map(|v| {
        let invalid = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
        let cleaned = v
            .chars()
            .map(|c| if invalid.contains(&c) { '_' } else { c })
            .collect::<String>();
        let squashed = cleaned
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("_")
            .trim()
            .to_string();
        if squashed.is_empty()
        {
            "unknown".to_string()
        }
        else
        {
            squashed
        }
    })
}

fn build_note_preview(text: &str) -> String
{
    const MAX_PREVIEW_CHARS: usize = 120;

    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty()
    {
        return String::new();
    }

    if collapsed.chars().count() <= MAX_PREVIEW_CHARS
    {
        return collapsed;
    }

    let mut preview = collapsed
        .chars()
        .take(MAX_PREVIEW_CHARS)
        .collect::<String>();
    preview.push_str("...");
    preview
}

fn escape_html_with_line_breaks(text: &str) -> String
{
    escape_html(text)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "<br>")
}

fn summary_state_and_background(
    current_ms: Option<f64>,
    baseline_ms: Option<f64>,
    is_baseline_group: bool,
) -> (&'static str, &'static str)
{
    if is_baseline_group
    {
        return (
            "baseline",
            "linear-gradient(90deg, rgba(107, 163, 208, 0.24), rgba(107, 163, 208, 0.07))",
        );
    }

    match (current_ms, baseline_ms)
    {
        (Some(current), Some(base)) if current < base => (
            "better",
            "linear-gradient(90deg, rgba(124, 197, 118, 0.32), rgba(124, 197, 118, 0.09))",
        ),
        (Some(current), Some(base)) if current > base => (
            "worse",
            "linear-gradient(90deg, rgba(255, 107, 107, 0.32), rgba(255, 107, 107, 0.09))",
        ),
        _ => (
            "equal",
            "linear-gradient(90deg, rgba(107, 163, 208, 0.22), rgba(107, 163, 208, 0.06))",
        ),
    }
}

fn percentile_sorted(values: &[f64], p: f64) -> Option<f64>
{
    if values.is_empty()
    {
        return None;
    }

    let n = values.len();
    let rank = p.clamp(0.0, 1.0) * (n.saturating_sub(1) as f64);
    let low = rank.floor() as usize;
    let high = rank.ceil() as usize;

    if low == high
    {
        return values.get(low).copied();
    }

    let frac = rank - low as f64;
    let a = values.get(low).copied()?;
    let b = values.get(high).copied()?;
    Some(a + (b - a) * frac)
}

fn mean(values: &[f64]) -> f64
{
    if values.is_empty()
    {
        0.0
    }
    else
    {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn round_to_scaled_bin(value: f64, step: f64) -> i64 { (value / step).round() as i64 }

fn scaled_bin_to_f64(scaled: i64, step: f64) -> f64 { scaled as f64 * step }

fn rgba(hex_color: &str, alpha: f64) -> String
{
    let hex = hex_color.trim_start_matches('#');
    if hex.len() != 6
    {
        return format!("rgba(255,255,255,{alpha})");
    }

    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
    format!("rgba({r},{g},{b},{alpha})")
}

fn palette_colors() -> &'static [&'static str]
{
    &[
        "#636EFA", "#EF553B", "#00CC96", "#AB63FA", "#FFA15A", "#19D3F3", "#FF6692", "#B6E880",
    ]
}

fn escape_html(text: &str) -> String
{
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn choose_diff_color(baseline: &str, current: &str, baseline_number: Option<f64>) -> &'static str
{
    if baseline.is_empty() && !current.is_empty()
    {
        return "#7cc576";
    }
    if !baseline.is_empty() && current.is_empty()
    {
        return "#ff6b6b";
    }

    if let Some(base_num) = baseline_number
        && let Ok(current_num) = current.parse::<f64>()
    {
        if current_num > base_num
        {
            return "#7cc576";
        }
        if current_num < base_num
        {
            return "#ff6b6b";
        }
        return "#6ba3d0";
    }

    if baseline == current
    {
        "#6ba3d0"
    }
    else
    {
        "#ff6b6b"
    }
}

fn format_metric_with_indicator(
    current_ms: Option<f64>,
    baseline_ms: Option<f64>,
    include_indicator: bool,
) -> String
{
    let Some(current_ms) = current_ms
    else
    {
        return "<span class=\"summary-metric-block\">N/A</span>".to_string();
    };

    let fps = if current_ms > 0.0
    {
        1000.0 / current_ms
    }
    else
    {
        0.0
    };

    let relative_perf_text = if include_indicator
    {
        match baseline_ms
        {
            Some(base_ms) if base_ms > 0.0 && current_ms > 0.0 =>
            {
                let relative_perf_pct = ((base_ms / current_ms) - 1.0) * 100.0;
                format!("{:+.1}% perf", relative_perf_pct)
            }
            _ => "N/A perf".to_string(),
        }
    }
    else
    {
        "100% perf".to_string()
    };

    let (state_class, marker) = match baseline_ms
    {
        Some(base) if current_ms < base => ("better", "▲"),
        Some(base) if current_ms > base => ("worse", "▼"),
        _ => ("equal", "="),
    };

    if include_indicator
    {
        format!(
            "<span class=\"summary-metric-block\"><span class=\"perf-indicator-{}\">{}</span><span><span class=\"summary-delta\">{}</span><br>{:.2} ms<br>{:.1} fps</span></span>",
            state_class, marker, relative_perf_text, current_ms, fps
        )
    }
    else
    {
        format!(
            "<span class=\"summary-metric-block\"><span class=\"summary-delta\">{}</span><br>{:.2} ms<br>{:.1} fps</span>",
            relative_perf_text, current_ms, fps
        )
    }
}

fn format_elapsed(duration: std::time::Duration) -> String
{
    let total_millis = duration.as_millis();
    let seconds = total_millis / 1_000;
    let millis = total_millis % 1_000;

    if seconds > 0
    {
        format!("{seconds}.{millis:03}s")
    }
    else
    {
        format!("{millis}ms")
    }
}

fn main()
{
    let args = CliArgs::parse();

    let timer_start = Instant::now();
    let chart_html_path = generate_chart(&args.benchmark_results_path, &args.bench_run_folder_name)
        .expect("Couldn't generate chart");

    // open the generated chart in the default web browser
    match open::that(&chart_html_path)
    {
        Ok(_) => println!(
            "Chart opened in default browser after {}.",
            format_elapsed(timer_start.elapsed())
        ),
        Err(err) => eprintln!("Failed to open chart in browser: {err:?}"),
    }
}
