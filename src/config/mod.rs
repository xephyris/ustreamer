use clap::Parser;

#[derive(Parser, Debug)]
#[command(about, long_about = None)]
pub struct Args {

    #[arg(short = 'v', long = "version")]
    pub show_version: bool,

    #[arg(short = 'f', long = "features")]
    pub features: bool,

    #[arg(short = 'd', long = "device", default_value = "/dev/video0")]
    pub device: String,

    #[arg(short = 'e', long = "drop-same-frames")]
    pub drop_frames: bool,

    #[arg(long = "exit-on-parent-death")]
    pub exit_on_parent_death: bool,
}