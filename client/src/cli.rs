use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use usb_bridge_protocol::{Session, UsbDevice};

use crate::{
    api::ApiClient,
    config::{self, Overrides},
    error::{ClientError, Result},
    usbip::{CommandOutput, UsbipRunner, WindowsUsbip},
};

#[derive(Parser)]
#[command(
    name = "usb-bridge-client",
    version,
    about = "LAN USB Bridge Windows client"
)]
struct Cli {
    #[arg(long, global = true, value_name = "URL")]
    server_url: Option<String>,
    #[arg(long, global = true)]
    client_id: Option<String>,
    #[arg(long, global = true, value_name = "PATH")]
    usbip_path: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "usbip.exeを実行せず、予定コマンドを表示する"
    )]
    dry_run: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Health,
    Devices,
    Session,
    Acquire(DeviceIds),
    Release,
    Usbip {
        #[command(subcommand)]
        command: UsbipCommand,
    },
}

#[derive(Args)]
struct DeviceIds {
    #[arg(value_name = "BUS_ID")]
    devices: Vec<String>,
}

#[derive(Subcommand)]
enum UsbipCommand {
    Status,
    List,
    Attach {
        #[arg(value_name = "BUS_ID")]
        bus_id: String,
    },
    Detach {
        #[arg(value_name = "BUS_ID")]
        bus_id: String,
    },
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let cfg = config::load(Overrides {
        server_url: cli.server_url,
        client_id: cli.client_id,
        usbip_path: cli.usbip_path,
        dry_run: cli.dry_run,
    })?;
    let api = ApiClient::new(cfg.server_url.clone())?;
    let usbip = WindowsUsbip::new(cfg.usbip_path, cfg.dry_run);
    let host = cfg
        .server_url
        .host_str()
        .ok_or_else(|| ClientError::Config("サーバーURLにホストがありません".into()))?;

    match cli.command {
        Command::Health => {
            let value = api.health()?;
            println!("status: {}\nbackend: {}", value.status, value.backend);
        }
        Command::Devices => print_devices(&api.devices()?),
        Command::Session => print_session(api.session()?.session.as_ref()),
        Command::Acquire(ids) => {
            ensure_not_owned_by_other(&api, &cfg.client_id)?;
            let session = api.acquire(&cfg.client_id, ids.devices)?;
            print_session(Some(&session));
            println!(
                "注意: サーバーは現時点でUSB/IP bindを実行しません。これは利用権の取得のみです。"
            );
        }
        Command::Release => release_owned(&api, &cfg.client_id)?,
        Command::Usbip { command } => match command {
            UsbipCommand::Status => print_output(usbip.status()?),
            UsbipCommand::List => print_output(usbip.list(host)?),
            UsbipCommand::Attach { bus_id } => {
                attach(&api, &usbip, host, &cfg.client_id, &bus_id, cfg.dry_run)?
            }
            UsbipCommand::Detach { bus_id } => {
                let session = ensure_owned(&api, &cfg.client_id)?;
                if !session.devices.iter().any(|id| id == &bus_id) {
                    return Err(ClientError::DeviceUnavailable(bus_id));
                }
                warn_for_device(&api.devices()?, &bus_id);
                print_output(usbip.detach_bus_id(&bus_id)?);
                release_owned(&api, &cfg.client_id)?;
            }
        },
    }
    Ok(())
}

fn attach(
    api: &ApiClient,
    usbip: &impl UsbipRunner,
    host: &str,
    client_id: &str,
    bus_id: &str,
    dry_run: bool,
) -> Result<()> {
    ensure_not_owned_by_other(api, client_id)?;
    let devices = api.devices()?;
    let device = devices
        .iter()
        .find(|d| d.bus_id == bus_id && d.selectable)
        .ok_or_else(|| ClientError::DeviceUnavailable(bus_id.into()))?;
    warn_device(device);
    if dry_run {
        println!("dry-run: acquire client_id={client_id} BUS_ID={bus_id}");
        print_output(usbip.attach(host, bus_id)?);
        println!("dry-run: attach失敗時は取得したセッションをrelease");
        return Ok(());
    }
    api.acquire(client_id, vec![bus_id.into()])?;
    match usbip.attach(host, bus_id) {
        Ok(output) => {
            print_output(output);
            Ok(())
        }
        Err(error) => {
            eprintln!("attachに失敗したため、取得したセッションを解放します");
            if let Err(release_error) = api.release(client_id) {
                eprintln!("警告: セッションの自動解放にも失敗しました: {release_error}");
            }
            Err(error)
        }
    }
}

fn ensure_not_owned_by_other(api: &ApiClient, client_id: &str) -> Result<()> {
    if let Some(session) = api.session()?.session {
        return if session.client_id == client_id {
            Err(ClientError::SessionAlreadyExists)
        } else {
            Err(ClientError::SessionOwnedByOther(session.client_id))
        };
    }
    Ok(())
}

fn ensure_owned(api: &ApiClient, client_id: &str) -> Result<Session> {
    match api.session()?.session {
        Some(session) if session.client_id == client_id => Ok(session),
        Some(session) => Err(ClientError::SessionOwnedByOther(session.client_id)),
        None => Err(ClientError::Config(
            "解放・切断できるセッションがありません".into(),
        )),
    }
}

fn release_owned(api: &ApiClient, client_id: &str) -> Result<()> {
    ensure_owned(api, client_id)?;
    api.release(client_id)?;
    println!("セッションを解放しました");
    Ok(())
}

fn print_devices(devices: &[UsbDevice]) {
    if devices.is_empty() {
        println!("USBデバイスはありません");
        return;
    }
    for d in devices {
        let name = d.product.as_deref().unwrap_or("(製品名不明)");
        println!(
            "{}  {}:{}  {}  selectable={} risk={} status={}",
            d.bus_id, d.vendor_id, d.product_id, name, d.selectable, d.risk, d.status
        );
        if let Some(warning) = &d.warning {
            println!("  警告: {warning}");
        }
    }
}

fn print_session(session: Option<&Session>) {
    match session {
        Some(s) => println!(
            "client_id: {}\ndevices: {}",
            s.client_id,
            if s.devices.is_empty() {
                "(なし)".into()
            } else {
                s.devices.join(", ")
            }
        ),
        None => println!("アクティブなセッションはありません"),
    }
}

fn warn_for_device(devices: &[UsbDevice], bus_id: &str) {
    if let Some(device) = devices.iter().find(|d| d.bus_id == bus_id) {
        warn_device(device);
    }
}

fn warn_device(device: &UsbDevice) {
    if device.risk != "normal"
        || device
            .interface_classes
            .iter()
            .any(|class| class.eq_ignore_ascii_case("08"))
    {
        eprintln!(
            "警告: {} は切断時にデータ損失や機能停止の可能性があります。書き込みを停止し、安全に取り外せる状態を確認してください。",
            device.bus_id
        );
    }
    if let Some(warning) = &device.warning {
        eprintln!("警告: {warning}");
    }
}

fn print_output(output: CommandOutput) {
    if !output.stdout.is_empty() {
        println!("{}", output.stdout);
    }
    if !output.stderr.is_empty() {
        eprintln!("{}", output.stderr);
    }
}
