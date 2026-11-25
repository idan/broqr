use std::{path::PathBuf, process::ExitCode};

use broqr_core::{check_printer, prepare_file, print_label, NetworkOptions, PrinterAddress};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "broqr",
    version,
    about = "Crisp image labels for Brother QL-810W / 62 mm continuous tape"
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Prepare a label without contacting a printer.
    Prepare {
        image: PathBuf,
        /// Save the exact black-and-white print artwork as PNG.
        #[arg(long)]
        preview: Option<PathBuf>,
        /// Save raw Brother raster commands for inspection or later printing.
        #[arg(long)]
        raster: Option<PathBuf>,
    },
    /// Print one label and cut it. No automatic retries.
    Print {
        image: PathBuf,
        #[arg(long)]
        printer: PrinterAddress,
    },
    /// Test connection and check printer/media status without printing.
    Check {
        #[arg(long)]
        printer: PrinterAddress,
    },
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    match args.command {
        Command::Prepare {
            image,
            preview,
            raster,
        } => {
            let label = prepare_file(image)?;
            let layout = label.layout();
            println!(
                "{} × {} dots · {:.1} mm artwork length · approximately {:.1} mm cut length{}",
                layout.width_dots,
                layout.height_dots,
                layout.artwork_length_mm,
                layout.label_length_mm,
                if layout.rotated {
                    " · rotated 90°"
                } else {
                    ""
                }
            );
            if let Some(path) = preview {
                std::fs::write(&path, label.preview_png()?)?;
                println!("Preview: {}", path.display());
            }
            if let Some(path) = raster {
                std::fs::write(&path, label.raster_bytes())?;
                println!("Raster commands: {}", path.display());
            }
        }
        Command::Print { image, printer } => {
            let label = prepare_file(image)?;
            let result = print_label(printer, &label, NetworkOptions::default())?;
            println!("{}", result.message);
            if !result.confirmed {
                eprintln!("Do not retry without checking the physical printer.");
            }
        }
        Command::Check { printer } => {
            println!(
                "{}",
                check_printer(printer, NetworkOptions::default())?.message
            );
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("BroQR: {error}");
            ExitCode::FAILURE
        }
    }
}
