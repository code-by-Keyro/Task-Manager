//! Docker polling and explicitly confirmed container actions on a separate worker.
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub state: String,
    pub status: String,
    pub image: String,
    pub project: String,
    pub cpu: Option<f64>,
    pub memory: Option<(u64, u64)>,
    pub network: Option<(u64, u64)>,
    pub disk: Option<(u64, u64)>,
    pub pids: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub containers: Vec<Container>,
    pub context: String,
    pub cpus: Option<usize>,
    pub error: Option<String>,
    pub updated: Instant,
    pub target: Option<Target>,
}

#[derive(Clone, Debug)]
pub struct Target {
    selector: String,
    label: String,
    context: Option<String>,
    fingerprint: String,
}
impl Target {
    fn current() -> Result<Self, String> {
        let explicit = std::env::var("DOCKER_CONTEXT")
            .ok()
            .filter(|s| !s.is_empty());
        if explicit.is_none()
            && let Some(host) = std::env::var("DOCKER_HOST").ok().filter(|s| !s.is_empty())
        {
            return Ok(Self {
                selector: format!("--host={host}"),
                label: "DOCKER_HOST".into(),
                context: None,
                fingerprint: String::new(),
            });
        }
        let context = match explicit {
            Some(c) => c,
            None => run("docker", &["context", "show"], Duration::from_secs(4))?
                .trim()
                .to_owned(),
        };
        if context.is_empty() {
            return Err(crate::tr!("No Docker context available.").into());
        }
        let fingerprint = run(
            "docker",
            &[
                "context",
                "inspect",
                &context,
                "--format",
                "{{json .Endpoints.docker}}",
            ],
            Duration::from_secs(4),
        )?
        .trim()
        .to_owned();
        Ok(Self {
            selector: format!("--context={context}"),
            label: context.clone(),
            context: Some(context),
            fingerprint,
        })
    }
    fn args(&self, args: &[&str]) -> Vec<String> {
        std::iter::once(self.selector.clone())
            .chain(args.iter().map(|s| (*s).to_owned()))
            .collect()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Stop,
    Restart,
}
impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stop => crate::tr!("Stop"),
            Self::Restart => crate::tr!("Restart"),
        }
    }
    fn command(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Restart => "restart",
        }
    }
    pub fn enabled(self, state: &str) -> bool {
        match self {
            Self::Stop => state == "running",
            Self::Restart => matches!(state, "running" | "exited" | "created"),
        }
    }
}
pub struct Action {
    pub target: Target,
    pub container: Container,
    pub operation: Operation,
    pub queued: Instant,
}
pub struct ActionResult {
    pub name: String,
    pub operation: Operation,
    pub result: Result<String, String>,
}

fn cli(args: &[String], timeout: Duration) -> Result<String, String> {
    run(
        "docker",
        &args.iter().map(String::as_str).collect::<Vec<_>>(),
        timeout,
    )
}
/// The adapter tests validation and exact argv without touching any real container.
pub fn apply_with(
    action: &Action,
    mut query: impl FnMut(&[String], Duration) -> Result<String, String>,
) -> Result<String, String> {
    let id = &action.container.id;
    if id.len() != 64 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(crate::tr!("Invalid container ID.").into());
    }
    if action.queued.elapsed() > Duration::from_secs(30) {
        return Err(crate::tr!("The action is out of date. Please refresh.").into());
    }
    if let Some(context) = &action.target.context {
        let current = query(
            &action.target.args(&[
                "context",
                "inspect",
                context,
                "--format",
                "{{json .Endpoints.docker}}",
            ]),
            Duration::from_secs(4),
        )?;
        if current.trim() != action.target.fingerprint {
            return Err(crate::tr!("The Docker target has changed. Please refresh again.").into());
        }
    }
    let inspect = query(
        &action.target.args(&[
            "container",
            "inspect",
            "--format",
            "{{.Id}}\t{{.State.Status}}",
            id,
        ]),
        Duration::from_secs(4),
    )?;
    let (actual, state) = inspect
        .trim()
        .split_once('\t')
        .ok_or(crate::tr!("Unable to check container state."))?;
    if actual != id || !action.operation.enabled(state) {
        return Err(crate::tr!(
            "The original container is no longer in the required state. Please refresh."
        )
        .into());
    }
    let output=query(&action.target.args(&["container",action.operation.command(),"--timeout","10",id]),Duration::from_secs(45))
        .map_err(|error|crate::tf!("{0}\nAfter a connection error, Docker may already be processing the request. Refresh before trying again." , error))?;
    if output.trim() != id {
        return Err(crate::tr!(
            "Docker returned an unexpected response. Please refresh the status."
        )
        .into());
    }
    Ok(match action.operation {
        Operation::Stop => crate::tr!("Docker stopped the container."),
        Operation::Restart => {
            crate::tr!("Docker restarted the container. Its current status will be checked again.")
        }
    }
    .into())
}

pub fn run(command: &str, args: &[&str], timeout: Duration) -> Result<String, String> {
    crate::command::run(command, args, timeout, "Docker")
}

fn bytes(text: &str) -> Option<u64> {
    let text = text.trim();
    let split = text.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let number = text[..split].parse::<f64>().ok()?;
    let scale: f64 = match text[split..].trim() {
        "B" => 1.0,
        "kB" | "KB" => 1000.0,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        "KiB" => 1024.0,
        "MiB" => 1024.0_f64.powi(2),
        "GiB" => 1024.0_f64.powi(3),
        "TiB" => 1024.0_f64.powi(4),
        _ => return None,
    };
    let value = number * scale;
    (value.is_finite() && value >= 0.0 && value < u64::MAX as f64).then(|| value.round() as u64)
}
fn pair(text: &str) -> Option<(u64, u64)> {
    let (first, second) = text.split_once('/')?;
    Some((bytes(first)?, bytes(second)?))
}

pub fn parse(list: &str, stats: &str) -> Result<Vec<Container>, String> {
    let mut containers = HashMap::new();
    for line in list.lines().filter(|l| !l.trim().is_empty()) {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 6
            || fields[0].len() != 64
            || !fields[0].chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err(crate::tr!("Unable to read the Docker container list.").into());
        }
        containers.insert(
            fields[0].to_owned(),
            Container {
                id: fields[0].into(),
                name: fields[1].into(),
                state: fields[2].into(),
                status: fields[3].into(),
                image: fields[4].into(),
                project: fields[5].into(),
                ..Default::default()
            },
        );
    }
    for line in stats.lines().filter(|l| !l.trim().is_empty()) {
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 6 {
            return Err(crate::tr!("Unable to read Docker metrics.").into());
        }
        let Some(container) = containers.get_mut(fields[0]) else {
            continue;
        };
        if container.state != "running" {
            continue;
        }
        container.cpu = fields[1]
            .trim_end_matches('%')
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && *n >= 0.0);
        container.memory = pair(fields[2]);
        container.network = pair(fields[3]);
        container.disk = pair(fields[4]);
        container.pids = fields[5].parse().ok();
    }
    let mut rows: Vec<_> = containers.into_values().collect();
    rows.sort_by(|a, b| {
        (a.state != "running", a.name.to_lowercase())
            .cmp(&(b.state != "running", b.name.to_lowercase()))
    });
    Ok(rows)
}

pub fn sample() -> Snapshot {
    let mut snapshot = Snapshot {
        containers: vec![],
        context: "—".into(),
        cpus: None,
        error: None,
        updated: Instant::now(),
        target: None,
    };
    let target = match Target::current() {
        Ok(target) => target,
        Err(error) => {
            snapshot.error = Some(error);
            return snapshot;
        }
    };
    snapshot.context = target.label.clone();
    snapshot.target = Some(target.clone());
    let timeout = Duration::from_secs(4);
    let list=match cli(&target.args(&["container","ls","--all","--no-trunc","--format","{{.ID}}\t{{.Names}}\t{{.State}}\t{{.Status}}\t{{.Image}}\t{{.Label \"com.docker.compose.project\"}}"]),timeout) {
        Ok(list)=>list,Err(error)=>{snapshot.error=Some(error);return snapshot;}
    };
    snapshot.cpus = cli(&target.args(&["info", "--format", "{{.NCPU}}"]), timeout)
        .ok()
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|n| *n > 0);
    match parse(&list, "") {
        Ok(rows) => snapshot.containers = rows,
        Err(error) => {
            snapshot.error = Some(error);
            return snapshot;
        }
    }
    if snapshot.containers.iter().any(|c| c.state == "running") {
        match cli(
            &target.args(&[
                "stats",
                "--no-stream",
                "--no-trunc",
                "--format",
                "{{.ID}}\t{{.CPUPerc}}\t{{.MemUsage}}\t{{.NetIO}}\t{{.BlockIO}}\t{{.PIDs}}",
            ]),
            timeout,
        )
        .and_then(|stats| parse(&list, &stats))
        {
            Ok(rows) => snapshot.containers = rows,
            Err(error) => snapshot.error = Some(error),
        }
    }
    snapshot.updated = Instant::now();
    snapshot
}

pub enum Control {
    Pause(bool),
    Refresh,
    Action(Box<Action>),
    Stop,
}
pub struct Monitor {
    pub control: Sender<Control>,
    pub receiver: Receiver<Snapshot>,
    pub action_receiver: Receiver<ActionResult>,
    thread: Option<JoinHandle<()>>,
}
impl Monitor {
    pub fn start(ctx: eframe::egui::Context) -> Self {
        let (control, commands) = mpsc::channel();
        let (sender, receiver) = mpsc::sync_channel(1);
        let (results, action_receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            let mut paused = true;
            loop {
                if !paused {
                    let snapshot = sample();
                    match sender.try_send(snapshot) {
                        Ok(()) | Err(mpsc::TrySendError::Full(_)) => ctx.request_repaint(),
                        Err(mpsc::TrySendError::Disconnected(_)) => break,
                    }
                }
                let command = if paused {
                    match commands.recv() {
                        Ok(c) => Some(c),
                        Err(_) => break,
                    }
                } else {
                    match commands.recv_timeout(Duration::from_secs(3)) {
                        Ok(c) => Some(c),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                };
                match command {
                    Some(Control::Stop) => break,
                    Some(Control::Pause(value)) => paused = value,
                    Some(Control::Refresh) => paused = false,
                    Some(Control::Action(action)) => {
                        let result = apply_with(&action, cli);
                        let _ = results.send(ActionResult {
                            name: action.container.name,
                            operation: action.operation,
                            result,
                        });
                        ctx.request_repaint();
                        // Sample next iteration unless the user has paused/left this page.
                    }
                    None => {}
                }
            }
        });
        Self {
            control,
            receiver,
            action_receiver,
            thread: Some(thread),
        }
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.control.send(Control::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn action(operation: Operation) -> Action {
        Action {
            target: Target {
                selector: "--context=test".into(),
                label: "test".into(),
                context: Some("test".into()),
                fingerprint: "endpoint".into(),
            },
            container: Container {
                id: "a".repeat(64),
                name: "test container".into(),
                ..Default::default()
            },
            operation,
            queued: Instant::now(),
        }
    }
    #[test]
    fn confirmed_actions_pin_context_full_id_and_grace_timeout_without_real_mutations() {
        for operation in [Operation::Stop, Operation::Restart] {
            let action = action(operation);
            let mut calls = vec![];
            let result = apply_with(&action, |args, timeout| {
                assert_eq!(args[0], "--context=test");
                calls.push(args.to_vec());
                if args[1] == "context" {
                    Ok("endpoint\n".into())
                } else if args[2] == "inspect" {
                    Ok(format!("{}\trunning\n", action.container.id))
                } else {
                    assert_eq!(timeout, Duration::from_secs(45));
                    Ok(action.container.id.clone())
                }
            })
            .unwrap();
            assert!(!result.is_empty());
            assert_eq!(calls.len(), 3);
            assert_eq!(
                calls[2],
                vec![
                    "--context=test",
                    "container",
                    operation.command(),
                    "--timeout",
                    "10",
                    &action.container.id
                ]
            );
        }
    }
    #[test]
    fn changed_endpoint_removed_replaced_stopped_or_stale_containers_never_receive_commands() {
        let action = action(Operation::Stop);
        let mut calls = 0;
        assert!(
            apply_with(&action, |_, _| {
                calls += 1;
                Ok("different endpoint".into())
            })
            .is_err()
        );
        assert_eq!(calls, 1);
        for inspect in [
            format!("{}\trunning", "b".repeat(64)),
            format!("{}\texited", action.container.id),
            "bad output".into(),
        ] {
            let mut calls = 0;
            assert!(
                apply_with(&action, |_, _| {
                    calls += 1;
                    assert!(calls <= 2, "mutation after failed check");
                    Ok(if calls == 1 {
                        "endpoint".into()
                    } else {
                        inspect.clone()
                    })
                })
                .is_err()
            );
        }
        let mut stale = action;
        stale.queued = Instant::now() - Duration::from_secs(31);
        assert!(apply_with(&stale, |_, _| panic!("stale query")).is_err());
        stale.container.id = "--malicious-option".into();
        stale.queued = Instant::now();
        assert!(apply_with(&stale, |_, _| panic!("invalid ID query")).is_err());
        assert!(Operation::Restart.enabled("exited"));
        assert!(!Operation::Stop.enabled("exited"));
    }
    #[test]
    fn container_identity_units_missing_stats_and_stopped_rows() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let list = format!(
            "{a}\tweb\trunning\tUp 2 hours\tweb:1\tstack\n{b}\told\texited\tExited (0)\told:2\t\n"
        );
        let stats = format!(
            "{a}\t250.50%\t1.5GiB / 4GiB\t2MB / 4kB\t3GB / 2MiB\t7\n{b}\t0.00%\t0B / 0B\t0B / 0B\t0B / 0B\t0\n"
        );
        let rows = parse(&list, &stats).unwrap();
        assert_eq!(rows[0].cpu, Some(250.5));
        assert_eq!(rows[0].memory, Some((1610612736, 4294967296)));
        assert_eq!(rows[0].network, Some((2000000, 4000)));
        assert_eq!(rows[0].project, "stack");
        assert!(rows[1].memory.is_none());
        assert!(parse(&list, "").unwrap()[0].cpu.is_none());
        assert!(parse("bad", "").is_err());
        assert!(bytes("-1GiB").is_none());
        assert!(bytes("NaNGiB").is_none());
        assert!(pair("1MiB / ???").is_none());
    }
    #[test]
    fn query_timeout_only_terminates_its_disposable_child_and_drains_output() {
        assert!(
            run("/bin/sleep", &["2"], Duration::from_millis(40))
                .unwrap_err()
                .contains("lange")
        );
        let output = run(
            "/usr/bin/head",
            &["-c", "100000", "/dev/zero"],
            Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(output.len(), 100000);
        let start = Instant::now();
        assert!(
            run(
                "/bin/sh",
                &["-c", "sleep 2 & wait"],
                Duration::from_millis(40)
            )
            .is_err()
        );
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "inherited pipe survived timeout"
        );
    }
}
