use std::{path::PathBuf, sync::atomic::AtomicBool};

use clap::{Args, Parser, Subcommand};
use usb_bridge_protocol::{Session, UsbDevice};

use usb_bridge_client_core::{
    api::ApiClient,
    config::{self, Overrides},
    connection,
    device_policy::{ensure_allowed, evaluate, is_ftdi, prohibited_class},
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
    let usbip = WindowsUsbip::new(cfg.usbip_path.clone(), cfg.dry_run);
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
            validate_acquire_devices(&api.devices()?, &ids.devices)?;
            let session = api.acquire(&cfg.client_id, ids.devices)?;
            print_session(Some(&session));
            println!(
                "サーバーがhost-agent制御を有効にしている場合、対象デバイスはUSB/IPへexportされています。"
            );
        }
        Command::Release => release_owned(&api, &cfg.client_id)?,
        Command::Usbip { command } => match command {
            UsbipCommand::Status => print_output(usbip.status()?),
            UsbipCommand::List => print_output(usbip.list(host)?),
            UsbipCommand::Attach { bus_id } => attach(&api, &usbip, &cfg, &bus_id, cfg.dry_run)?,
            UsbipCommand::Detach { bus_id } => {
                let session = ensure_owned(&api, &cfg.client_id)?;
                if !session.devices.iter().any(|id| id == &bus_id) {
                    return Err(ClientError::DeviceUnavailable(bus_id));
                }
                warn_for_device(&api.devices()?, &bus_id);
                connection::disconnect_group(&cfg, &usbip, &[bus_id], |message| {
                    println!("{message}")
                })?;
            }
        },
    }
    Ok(())
}

fn attach(
    api: &ApiClient,
    usbip: &impl UsbipRunner,
    config: &config::Config,
    bus_id: &str,
    dry_run: bool,
) -> Result<()> {
    let devices = api.devices()?;
    let device = devices
        .iter()
        .find(|d| d.bus_id == bus_id)
        .ok_or_else(|| ClientError::DeviceUnavailable(bus_id.into()))?;
    ensure_allowed(device)?;
    if !device.selectable {
        return Err(ClientError::DeviceUnavailable(bus_id.into()));
    }
    warn_device(device);
    if dry_run {
        let host = config
            .server_url
            .host_str()
            .ok_or_else(|| ClientError::Config("サーバーURLにホストがありません".into()))?;
        println!(
            "dry-run: acquire client_id={} BUS_ID={bus_id}",
            config.client_id
        );
        print_output(usbip.attach(host, bus_id)?);
        println!("dry-run: attach失敗時は取得したセッションをrelease");
        return Ok(());
    }
    let cancelled = AtomicBool::new(false);
    connection::connect_group(
        config,
        usbip,
        std::slice::from_ref(device),
        &cancelled,
        |message| println!("{message}"),
    )?;
    Ok(())
}

fn ensure_not_owned_by_other(api: &ApiClient, client_id: &str) -> Result<()> {
    if let Some(session) = api.session()?.session
        && session.client_id != client_id
    {
        return Err(ClientError::SessionOwnedByOther(session.client_id));
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
        let policy = evaluate(d);
        println!(
            "[{}] {}  {}:{}  {}  selectable={} risk={} status={}",
            policy.label, d.bus_id, d.vendor_id, d.product_id, name, d.selectable, d.risk, d.status
        );
        if let Some(class_name) = prohibited_class(d) {
            println!("  禁止理由: {class_name}クラスは安全上の理由により転送対象外です");
        }
        if is_ftdi(d) {
            println!("  WARNING: FTDI系デバイスはusbip-win2で互換性問題が報告されています");
        }
        if prohibited_class(d).is_none()
            && !is_ftdi(d)
            && let Some(warning) = &d.warning
        {
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
    if is_ftdi(device) {
        eprintln!(
            "WARNING: {} はFTDI系デバイスです。usbip-win2では列挙失敗やSTATUS_NO_SUCH_DEVICEなどの互換性問題が報告されています。",
            device.bus_id
        );
    }
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

fn validate_acquire_devices(devices: &[UsbDevice], requested: &[String]) -> Result<()> {
    let targets: Vec<&UsbDevice> = if requested.is_empty() {
        devices.iter().filter(|device| device.selected).collect()
    } else {
        requested
            .iter()
            .map(|bus_id| {
                devices
                    .iter()
                    .find(|device| &device.bus_id == bus_id)
                    .ok_or_else(|| ClientError::DeviceUnavailable(bus_id.clone()))
            })
            .collect::<Result<_>>()?
    };
    for device in targets {
        ensure_allowed(device)?;
        if !device.selectable {
            return Err(ClientError::DeviceUnavailable(device.bus_id.clone()));
        }
        warn_device(device);
    }
    Ok(())
}

fn print_output(output: CommandOutput) {
    if !output.stdout.is_empty() {
        println!("{}", output.stdout);
    }
    if !output.stderr.is_empty() {
        eprintln!("{}", output.stderr);
    }
}
