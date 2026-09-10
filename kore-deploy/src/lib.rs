use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;

// ─── Error type ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum KoreError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Ssh(String),
    K8s(String),
    Config(String),
}

impl fmt::Display for KoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KoreError::Io(e) => write!(f, "IO error: {}", e),
            KoreError::Json(e) => write!(f, "JSON error: {}", e),
            KoreError::Ssh(msg) => write!(f, "SSH error: {}", msg),
            KoreError::K8s(msg) => write!(f, "K8s error: {}", msg),
            KoreError::Config(msg) => write!(f, "Config error: {}", msg),
        }
    }
}

impl std::error::Error for KoreError {}

impl From<std::io::Error> for KoreError {
    fn from(e: std::io::Error) -> Self {
        KoreError::Io(e)
    }
}

impl From<serde_json::Error> for KoreError {
    fn from(e: serde_json::Error) -> Self {
        KoreError::Json(e)
    }
}

// ─── StandaloneManager ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub worker_binary: String,
    pub cores: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkerStatus {
    Pending,
    Running,
    Stopped,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerHandle {
    pub host: String,
    pub pid: Option<u32>,
    pub status: WorkerStatus,
    pub ssh_command: String,
}

pub struct StandaloneManager {
    hosts: Vec<HostConfig>,
}

impl StandaloneManager {
    pub fn new(hosts: Vec<HostConfig>) -> Self {
        Self { hosts }
    }

    /// Builds SSH commands to launch worker binaries on each host.
    /// Does NOT actually execute SSH — returns the handles with generated commands.
    pub fn launch_workers(&self, coord_addr: &str) -> Result<Vec<WorkerHandle>, KoreError> {
        if self.hosts.is_empty() {
            return Err(KoreError::Ssh("No hosts configured".to_string()));
        }

        let mut handles = Vec::new();
        for host_cfg in &self.hosts {
            let ssh_command = format!(
                "ssh -p {} {}@{} '{} --coordinator {} --cores {}'",
                host_cfg.port,
                host_cfg.user,
                host_cfg.host,
                host_cfg.worker_binary,
                coord_addr,
                host_cfg.cores,
            );

            handles.push(WorkerHandle {
                host: host_cfg.host.clone(),
                pid: None,
                status: WorkerStatus::Pending,
                ssh_command,
            });
        }

        Ok(handles)
    }

    /// Generates kill commands for remote processes.
    pub fn stop_workers(&self, handles: &[WorkerHandle]) -> Vec<String> {
        let mut kill_commands = Vec::new();
        for handle in handles {
            if let Some(pid) = handle.pid {
                if let Some(host_cfg) = self.hosts.iter().find(|h| h.host == handle.host) {
                    let cmd = format!(
                        "ssh -p {} {}@{} 'kill {}'",
                        host_cfg.port, host_cfg.user, host_cfg.host, pid
                    );
                    kill_commands.push(cmd);
                }
            }
        }
        kill_commands
    }

    pub fn hosts(&self) -> &[HostConfig] {
        &self.hosts
    }
}

// ─── K8sManager ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct K8sDeployment {
    pub namespace: String,
    pub deployment_name: String,
    pub replicas: usize,
    pub pod_specs: Vec<serde_json::Value>,
}

pub struct K8sManager {
    namespace: String,
    image: String,
    replicas: usize,
}

impl K8sManager {
    pub fn new(namespace: String, image: String, replicas: usize) -> Self {
        Self {
            namespace,
            image,
            replicas,
        }
    }

    /// Generates Pod specs as JSON for worker deployment.
    pub fn deploy(&self, coord_addr: &str) -> Result<K8sDeployment, KoreError> {
        if self.replicas == 0 {
            return Err(KoreError::K8s("Replica count must be > 0".to_string()));
        }

        let mut pod_specs = Vec::new();
        for i in 0..self.replicas {
            let pod_spec = serde_json::json!({
                "apiVersion": "v1",
                "kind": "Pod",
                "metadata": {
                    "name": format!("kore-worker-{}", i),
                    "namespace": self.namespace,
                    "labels": {
                        "app": "kore-worker",
                        "instance": i.to_string()
                    }
                },
                "spec": {
                    "containers": [{
                        "name": "kore-worker",
                        "image": self.image,
                        "command": ["kore-worker"],
                        "args": ["--coordinator", coord_addr],
                        "ports": [{
                            "containerPort": 9877,
                            "name": "worker"
                        }],
                        "resources": {
                            "requests": {
                                "cpu": "1",
                                "memory": "2Gi"
                            },
                            "limits": {
                                "cpu": "4",
                                "memory": "8Gi"
                            }
                        }
                    }],
                    "restartPolicy": "Always"
                }
            });
            pod_specs.push(pod_spec);
        }

        Ok(K8sDeployment {
            namespace: self.namespace.clone(),
            deployment_name: "kore-worker".to_string(),
            replicas: self.replicas,
            pod_specs,
        })
    }

    pub fn scale(&mut self, replicas: usize) {
        self.replicas = replicas;
    }

    pub fn teardown(&self) -> serde_json::Value {
        serde_json::json!({
            "action": "delete",
            "resource": "deployment",
            "name": "kore-worker",
            "namespace": self.namespace
        })
    }

    pub fn replicas(&self) -> usize {
        self.replicas
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn image(&self) -> &str {
        &self.image
    }
}

// ─── ClusterConfig ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeployMode {
    Standalone,
    Kubernetes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    pub mode: DeployMode,
    pub coordinator_addr: String,
    pub hosts: Option<Vec<HostConfig>>,
    pub k8s_namespace: Option<String>,
    pub k8s_image: Option<String>,
    pub k8s_replicas: Option<usize>,
}

impl ClusterConfig {
    pub fn from_file(path: &str) -> Result<Self, KoreError> {
        let content = fs::read_to_string(path)?;
        let config: ClusterConfig = serde_json::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    pub fn to_file(&self, path: &str) -> Result<(), KoreError> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    fn validate(&self) -> Result<(), KoreError> {
        if self.coordinator_addr.is_empty() {
            return Err(KoreError::Config(
                "coordinator_addr must not be empty".to_string(),
            ));
        }
        match self.mode {
            DeployMode::Standalone => {
                if self.hosts.as_ref().map_or(true, |h| h.is_empty()) {
                    return Err(KoreError::Config(
                        "Standalone mode requires at least one host".to_string(),
                    ));
                }
            }
            DeployMode::Kubernetes => {
                if self.k8s_namespace.is_none() || self.k8s_image.is_none() {
                    return Err(KoreError::Config(
                        "Kubernetes mode requires namespace and image".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_hosts() -> Vec<HostConfig> {
        vec![
            HostConfig {
                host: "worker-1.example.com".to_string(),
                port: 22,
                user: "kore".to_string(),
                worker_binary: "/usr/local/bin/kore-worker".to_string(),
                cores: 8,
            },
            HostConfig {
                host: "worker-2.example.com".to_string(),
                port: 2222,
                user: "admin".to_string(),
                worker_binary: "/opt/kore/kore-worker".to_string(),
                cores: 16,
            },
        ]
    }

    // ── StandaloneManager tests ──

    #[test]
    fn test_standalone_launch_generates_ssh_commands() {
        let mgr = StandaloneManager::new(sample_hosts());
        let handles = mgr.launch_workers("10.0.0.1:9876").unwrap();

        assert_eq!(handles.len(), 2);

        assert_eq!(handles[0].host, "worker-1.example.com");
        assert!(handles[0].ssh_command.contains("ssh -p 22"));
        assert!(handles[0].ssh_command.contains("kore@worker-1.example.com"));
        assert!(handles[0].ssh_command.contains("--coordinator 10.0.0.1:9876"));
        assert!(handles[0].ssh_command.contains("--cores 8"));

        assert_eq!(handles[1].host, "worker-2.example.com");
        assert!(handles[1].ssh_command.contains("ssh -p 2222"));
        assert!(handles[1].ssh_command.contains("admin@worker-2.example.com"));
        assert!(handles[1].ssh_command.contains("--cores 16"));
    }

    #[test]
    fn test_standalone_launch_empty_hosts_errors() {
        let mgr = StandaloneManager::new(vec![]);
        let result = mgr.launch_workers("10.0.0.1:9876");
        assert!(result.is_err());
    }

    #[test]
    fn test_standalone_worker_status_pending() {
        let mgr = StandaloneManager::new(sample_hosts());
        let handles = mgr.launch_workers("10.0.0.1:9876").unwrap();
        for handle in &handles {
            assert_eq!(handle.status, WorkerStatus::Pending);
            assert_eq!(handle.pid, None);
        }
    }

    #[test]
    fn test_standalone_stop_workers_generates_kill_commands() {
        let mgr = StandaloneManager::new(sample_hosts());
        let handles = vec![
            WorkerHandle {
                host: "worker-1.example.com".to_string(),
                pid: Some(12345),
                status: WorkerStatus::Running,
                ssh_command: String::new(),
            },
            WorkerHandle {
                host: "worker-2.example.com".to_string(),
                pid: Some(67890),
                status: WorkerStatus::Running,
                ssh_command: String::new(),
            },
        ];

        let kill_cmds = mgr.stop_workers(&handles);
        assert_eq!(kill_cmds.len(), 2);
        assert!(kill_cmds[0].contains("kill 12345"));
        assert!(kill_cmds[0].contains("worker-1.example.com"));
        assert!(kill_cmds[1].contains("kill 67890"));
        assert!(kill_cmds[1].contains("-p 2222"));
    }

    #[test]
    fn test_standalone_stop_skips_no_pid() {
        let mgr = StandaloneManager::new(sample_hosts());
        let handles = vec![WorkerHandle {
            host: "worker-1.example.com".to_string(),
            pid: None,
            status: WorkerStatus::Pending,
            ssh_command: String::new(),
        }];

        let kill_cmds = mgr.stop_workers(&handles);
        assert!(kill_cmds.is_empty());
    }

    // ── K8sManager tests ──

    #[test]
    fn test_k8s_deploy_generates_pod_specs() {
        let mgr = K8sManager::new("kore-prod".to_string(), "kore:latest".to_string(), 3);
        let deployment = mgr.deploy("10.0.0.1:9876").unwrap();

        assert_eq!(deployment.namespace, "kore-prod");
        assert_eq!(deployment.deployment_name, "kore-worker");
        assert_eq!(deployment.replicas, 3);
        assert_eq!(deployment.pod_specs.len(), 3);

        let first_pod = &deployment.pod_specs[0];
        assert_eq!(first_pod["kind"], "Pod");
        assert_eq!(first_pod["metadata"]["namespace"], "kore-prod");
        assert_eq!(first_pod["metadata"]["name"], "kore-worker-0");

        let container = &first_pod["spec"]["containers"][0];
        assert_eq!(container["image"], "kore:latest");
        assert_eq!(container["args"][1], "10.0.0.1:9876");
    }

    #[test]
    fn test_k8s_deploy_zero_replicas_errors() {
        let mgr = K8sManager::new("kore-prod".to_string(), "kore:latest".to_string(), 0);
        let result = mgr.deploy("10.0.0.1:9876");
        assert!(result.is_err());
    }

    #[test]
    fn test_k8s_scale() {
        let mut mgr = K8sManager::new("ns".to_string(), "img".to_string(), 2);
        assert_eq!(mgr.replicas(), 2);
        mgr.scale(5);
        assert_eq!(mgr.replicas(), 5);
    }

    #[test]
    fn test_k8s_teardown() {
        let mgr = K8sManager::new("kore-staging".to_string(), "img".to_string(), 1);
        let action = mgr.teardown();
        assert_eq!(action["action"], "delete");
        assert_eq!(action["namespace"], "kore-staging");
        assert_eq!(action["name"], "kore-worker");
    }

    #[test]
    fn test_k8s_pod_spec_structure() {
        let mgr = K8sManager::new("default".to_string(), "kore:v1".to_string(), 1);
        let deployment = mgr.deploy("coord:9876").unwrap();
        let pod = &deployment.pod_specs[0];

        assert_eq!(pod["apiVersion"], "v1");
        assert_eq!(pod["spec"]["restartPolicy"], "Always");
        assert_eq!(pod["spec"]["containers"][0]["ports"][0]["containerPort"], 9877);
    }

    // ── ClusterConfig tests ──

    #[test]
    fn test_cluster_config_roundtrip() {
        let config = ClusterConfig {
            mode: DeployMode::Standalone,
            coordinator_addr: "10.0.0.1:9876".to_string(),
            hosts: Some(sample_hosts()),
            k8s_namespace: None,
            k8s_image: None,
            k8s_replicas: None,
        };

        let tmp = std::env::temp_dir().join("kore_test_config.json");
        let path = tmp.to_str().unwrap();

        config.to_file(path).unwrap();
        let loaded = ClusterConfig::from_file(path).unwrap();

        assert_eq!(loaded.mode, DeployMode::Standalone);
        assert_eq!(loaded.coordinator_addr, "10.0.0.1:9876");
        assert_eq!(loaded.hosts.as_ref().unwrap().len(), 2);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_cluster_config_k8s_mode() {
        let config = ClusterConfig {
            mode: DeployMode::Kubernetes,
            coordinator_addr: "kore-coord.kore-prod.svc:9876".to_string(),
            hosts: None,
            k8s_namespace: Some("kore-prod".to_string()),
            k8s_image: Some("kore:latest".to_string()),
            k8s_replicas: Some(5),
        };

        let tmp = std::env::temp_dir().join("kore_test_k8s_config.json");
        let path = tmp.to_str().unwrap();

        config.to_file(path).unwrap();
        let loaded = ClusterConfig::from_file(path).unwrap();

        assert_eq!(loaded.mode, DeployMode::Kubernetes);
        assert_eq!(loaded.k8s_namespace, Some("kore-prod".to_string()));
        assert_eq!(loaded.k8s_replicas, Some(5));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_cluster_config_validation_empty_coordinator() {
        let json = r#"{"mode":"Standalone","coordinator_addr":"","hosts":[{"host":"h","port":22,"user":"u","worker_binary":"b","cores":1}]}"#;
        let tmp = std::env::temp_dir().join("kore_test_bad_coord.json");
        fs::write(&tmp, json).unwrap();

        let result = ClusterConfig::from_file(tmp.to_str().unwrap());
        assert!(result.is_err());

        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_cluster_config_validation_standalone_no_hosts() {
        let json = r#"{"mode":"Standalone","coordinator_addr":"addr:9876","hosts":[]}"#;
        let tmp = std::env::temp_dir().join("kore_test_no_hosts.json");
        fs::write(&tmp, json).unwrap();

        let result = ClusterConfig::from_file(tmp.to_str().unwrap());
        assert!(result.is_err());

        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_cluster_config_validation_k8s_missing_fields() {
        let json = r#"{"mode":"Kubernetes","coordinator_addr":"addr:9876","k8s_namespace":null,"k8s_image":null}"#;
        let tmp = std::env::temp_dir().join("kore_test_k8s_bad.json");
        fs::write(&tmp, json).unwrap();

        let result = ClusterConfig::from_file(tmp.to_str().unwrap());
        assert!(result.is_err());

        let _ = fs::remove_file(&tmp);
    }
}
