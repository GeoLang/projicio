use clap::Parser;
use projicio_core::Support;

#[derive(Parser)]
#[command(name = "projicio", about = "Coordinate transformation CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Transform coordinates between CRS
    Transform {
        /// Source CRS (e.g. EPSG:4326)
        #[arg(long)]
        from: String,
        /// Target CRS (e.g. EPSG:3857)
        #[arg(long)]
        to: String,
        /// X coordinate (or longitude)
        x: f64,
        /// Y coordinate (or latitude)
        y: f64,
    },
    /// Report whether a CRS is supported and which engine handles it
    Info {
        /// CRS to look up (e.g. EPSG:27700)
        crs: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Transform { from, to, x, y } => match projicio_core::Transform::new(&from, &to) {
            Ok(t) => match t.convert(x, y) {
                Ok((rx, ry)) => println!("{rx} {ry}"),
                Err(e) => eprintln!("Error: {e}"),
            },
            Err(e) => eprintln!("Error: {e}"),
        },
        Commands::Info { crs } => info(&crs),
    }
}

fn info(crs: &str) {
    let Some(code) = projicio_core::epsg::parse_wkt_epsg(crs).or_else(|| crs.parse().ok()) else {
        eprintln!("Error: could not read an EPSG code from {crs:?}");
        std::process::exit(1);
    };

    let support = projicio_core::epsg::support(code);
    println!("EPSG:{code}");
    match support {
        Support::Native => println!("support: native"),
        Support::Fallback => println!("support: fallback (proj4rs)"),
        Support::Unsupported => println!("support: none"),
    }
    if let Some(name) = projicio_core::epsg::lookup(code).map(|d| d.name) {
        println!("name: {name}");
    }
    if let Some(proj4) = projicio_core::epsg::proj4_definition(code) {
        println!("proj4: {proj4}");
    }
    if support == Support::Unsupported {
        std::process::exit(1);
    }
}
