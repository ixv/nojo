use crate::kernel::checkpoint::JamPaths;
use crate::kernel::form::Kernel;
use crate::{default_data_dir, NockApp};
use chrono;
use clap::{arg, command, ColorChoice, Parser};
use nockvm::jets::hot::HotEntry;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info, Level};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser, Debug, Clone)]
#[command(about = "boot a nockapp", author, version, color = ColorChoice::Auto)]
pub struct Cli {
    #[arg(
        long,
        help = "Start with a new data directory, removing any existing data",
        default_value = "false"
    )]
    pub new: bool,

    #[arg(long, help = "Make an Sword trace", default_value = "false")]
    pub trace: bool,

    #[arg(
        long,
        default_value = "1000",
        help = "Set the save interval for checkpoints (in ms)"
    )]
    pub save_interval: u64,

    #[arg(long, help = "Control colored output", value_enum, default_value_t = ColorChoice::Auto)]
    pub color: ColorChoice,

    #[arg(
        long,
        help = "Path to a jam file containing existing kernel state. Supports both JammedCheckpoint and ExportedState formats."
    )]
    pub state_jam: Option<String>,

    #[arg(
        long,
        help = "Path to export the kernel state as a jam file in the ExportedState format."
    )]
    pub export_state_jam: Option<String>,
}

/// Result of setting up a NockApp
pub enum SetupResult {
    /// A fully initialized NockApp
    App(NockApp),
    /// State was exported successfully
    ExportedState,
}

pub fn default_boot_cli(new: bool) -> Cli {
    Cli {
        save_interval: 1000,
        new,
        trace: false,
        color: ColorChoice::Auto,
        state_jam: None,
        export_state_jam: None,
    }
}

/// A minimal event formatter for development mode
struct MinimalFormatter;

impl<S, N> FormatEvent<S, N> for MinimalFormatter
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let level = *event.metadata().level();
        let level_str = match level {
            Level::TRACE => "\x1B[36mT\x1B[0m",
            Level::DEBUG => "\x1B[34mD\x1B[0m",
            Level::INFO => "\x1B[32mI\x1B[0m",
            Level::WARN => "\x1B[33mW\x1B[0m",
            Level::ERROR => "\x1B[31mE\x1B[0m",
        };

        // Get level color code for potential use with slogger
        let level_color = match level {
            Level::TRACE => "\x1B[36m", // Cyan
            Level::DEBUG => "\x1B[34m", // Blue
            Level::INFO => "\x1B[32m",  // Green
            Level::WARN => "\x1B[33m",  // Yellow
            Level::ERROR => "\x1B[31m", // Red
        };

        write!(writer, "{} ", level_str)?;

        // simple, shorter timestamp (HH:mm:ss)
        let now = chrono::Local::now();
        let time_str = now.format("%H:%M:%S").to_string();
        write!(writer, "\x1B[38;5;246m({time_str})\x1B[0m ")?;

        let target = event.metadata().target();

        // Special handling for slogger
        if target == "slogger" {
            // For slogger, omit the target prefix and color the message with the log level color
            // this mimics the behavior of slogging in urbit
            write!(writer, "{}", level_color)?;
            ctx.field_format().format_fields(writer.by_ref(), event)?;
            write!(writer, "\x1B[0m")?;

            return writeln!(writer);
        }

        let simplified_target = if target.contains("::") {
            // Just take the last component of the module path
            let parts: Vec<&str> = target.split("::").collect();
            if parts.len() > 1 {
                // If we have a structure like "a::b::c::d", just take "c::d"
                // but prefix it with the first two characters of the first part
                // i.e, nockapp::kernel::boot -> [cr] kernel::boot
                if parts.len() > 2 {
                    format!(
                        "[{}] {}::{}",
                        parts[0].chars().take(2).collect::<String>(),
                        parts[parts.len() - 2],
                        parts[parts.len() - 1]
                    )
                } else {
                    parts
                        .last()
                        .unwrap_or_else(|| {
                            panic!(
                                "Panicked at {}:{} (git sha: {:?})",
                                file!(),
                                line!(),
                                option_env!("GIT_SHA")
                            )
                        })
                        .to_string()
                }
            } else {
                target.to_string()
            }
        } else {
            target.to_string()
        };

        // Write the simplified target in grey and italics
        write!(writer, "\x1B[3;90m{}\x1B[0m: ", simplified_target)?;

        // Write the fields (the actual log message)
        ctx.field_format().format_fields(writer.by_ref(), event)?;

        writeln!(writer)
    }
}

/// Initialize tracing with appropriate configuration based on CLI arguments.
///
/// This function sets up logging with different profiles:
/// - Production mode: Full verbose logging as specified by log_level
/// - Development mode: Cleaner, less noisy logging focused on application code
///
/// In development mode, the base filter is set to INFO level, with application
/// modules set to DEBUG. Additional modules can be specified with dev_modules.
pub fn init_default_tracing(cli: &Cli) {
    let filter = EnvFilter::new(std::env::var("RUST_LOG").unwrap_or_else(|_| "trace".to_string()));
    let use_ansi = cli.color == ColorChoice::Auto || cli.color == ColorChoice::Always;

    // Build and initialize the subscriber based on format and mode
    match std::env::var("MINIMAL_LOG_FORMAT").unwrap_or_else(|_| "false".to_string()) == "true" {
        // Default pretty format for production
        false => {
            tracing_subscriber::registry()
                .with(
                    fmt::layer()
                        .with_ansi(use_ansi)
                        .with_target(true)
                        .with_level(true),
                )
                .with(filter)
                .init();
        }
        // Development mode with minimal formatter
        true => {
            let fmt_layer = fmt::layer()
                .with_ansi(use_ansi)
                .event_format(MinimalFormatter);

            tracing_subscriber::registry()
                .with(fmt_layer)
                .with(filter)
                .init();
        }
    }
}

pub async fn setup(
    jam: &[u8],
    cli: Option<Cli>,
    hot_state: &[HotEntry],
    name: &str,
    data_dir: Option<PathBuf>,
) -> Result<NockApp, Box<dyn std::error::Error>> {
    let result = setup_(
        jam,
        cli.unwrap_or_else(|| default_boot_cli(false)),
        hot_state,
        name,
        data_dir,
    )
    .await?;
    match result {
        SetupResult::App(app) => Ok(app),
        SetupResult::ExportedState => {
            info!("Exiting after successful state export");
            std::process::exit(0);
        }
    }
}

pub async fn setup_(
    jam: &[u8],
    cli: Cli,
    hot_state: &[HotEntry],
    name: &str,
    data_dir: Option<PathBuf>,
) -> Result<SetupResult, Box<dyn std::error::Error>> {
    let data_dir = if let Some(data_path) = data_dir.clone() {
        data_path.join(name)
    } else {
        default_data_dir(name)
    };
    let pma_dir = data_dir.join("pma");
    let jams_dir = data_dir.join("checkpoints");

    if !jams_dir.exists() {
        std::fs::create_dir_all(&jams_dir)?;
        debug!("Created jams directory: {:?}", jams_dir);
    }

    if pma_dir.exists() {
        std::fs::remove_dir_all(&pma_dir)?;
        debug!("Deleted existing pma directory: {:?}", pma_dir);
    }

    if cli.new && jams_dir.exists() {
        std::fs::remove_dir_all(&jams_dir)?;
        debug!("Deleted existing checkpoint directory: {:?}", jams_dir);
    }

    let jam_paths = JamPaths::new(&jams_dir);
    info!("kernel: starting");
    debug!("kernel: pma directory: {:?}", pma_dir);
    debug!(
        "kernel: jam buffer paths: {:?}, {:?}",
        jam_paths.0, jam_paths.1
    );

    let mut kernel = if let Some(state_path) = cli.state_jam {
        let state_bytes = fs::read(&state_path)?;
        debug!("kernel: loading state from jam file: {:?}", state_path);
        Kernel::load_with_kernel_state(pma_dir, jam_paths, jam, &state_bytes, hot_state, cli.trace)
            .await?
    } else {
        Kernel::load_with_hot_state(pma_dir, jam_paths, jam, hot_state, cli.trace).await?
    };

    if let Some(export_path) = cli.export_state_jam.clone() {
        export_kernel_state(&mut kernel, &export_path).await?;
        return Ok(SetupResult::ExportedState);
    }

    let save_interval = std::time::Duration::from_millis(cli.save_interval);

    let app = NockApp::new(kernel, save_interval).await;

    Ok(SetupResult::App(app))
}

/// Exports the kernel state to a jam file at the specified path
async fn export_kernel_state(
    kernel: &mut Kernel,
    export_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Extracting kernel state to file: {:?}", export_path);
    let state_bytes = kernel.create_state_bytes().await?;
    fs::write(export_path, state_bytes)?;
    info!("Successfully exported kernel state to: {:?}", export_path);
    Ok(())
}
