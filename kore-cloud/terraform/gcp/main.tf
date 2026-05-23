terraform {
  required_version = ">= 1.0"
  required_providers {
    google = {
      source  = "hashicorp/google"
      version = "~> 5.0"
    }
    google-beta = {
      source  = "hashicorp/google-beta"
      version = "~> 5.0"
    }
  }
}

provider "google" {
  project = var.gcp_project_id
  region  = var.gcp_region
}

provider "google-beta" {
  project = var.gcp_project_id
  region  = var.gcp_region
}

# ==================== Cloud SQL PostgreSQL ====================
resource "google_sql_database_instance" "kore" {
  name                = "kore-postgres-${var.environment}"
  database_version    = "POSTGRES_15"
  region              = var.gcp_region
  deletion_protection = var.environment == "prod"

  settings {
    tier              = var.environment == "prod" ? "db-custom-4-16384" : "db-f1-micro"
    availability_type = var.environment == "prod" ? "REGIONAL" : "ZONAL"
    backup_configuration {
      enabled                        = true
      start_time                     = "03:00"
      transaction_log_retention_days = 7
      backup_retention_settings {
        retained_backups = 30
        retention_unit   = "COUNT"
      }
    }

    database_flags {
      name  = "max_connections"
      value = "200"
    }

    database_flags {
      name  = "log_checkpoints"
      value = "on"
    }

    ip_configuration {
      require_ssl = true
      authorized_networks {
        name  = "Cloud Run"
        value = "0.0.0.0/0"
      }
    }

    insights_config {
      query_insights_enabled = var.environment == "prod"
    }
  }

  deletion_protection = false

  depends_on = [google_project_service.sqladmin]
}

resource "google_sql_database" "kore" {
  name     = "kore"
  instance = google_sql_database_instance.kore.name
}

resource "google_sql_user" "kore" {
  name     = "koremaster"
  instance = google_sql_database_instance.kore.name
  password = var.db_password
}

# ==================== Cloud Storage ====================
resource "google_storage_bucket" "kore" {
  name          = "kore-storage-${var.gcp_project_id}-${var.environment}"
  location      = var.gcp_region
  force_destroy = var.environment != "prod"

  uniform_bucket_level_access = true

  versioning {
    enabled = true
  }

  encryption {
    default_kms_key_name = var.environment == "prod" ? google_kms_crypto_key.storage.id : null
  }

  lifecycle_rule {
    condition {
      age = 90
    }
    action {
      type          = "SetStorageClass"
      storage_class = "STANDARD_IA"
    }
  }

  lifecycle_rule {
    condition {
      age = 365
    }
    action {
      type          = "SetStorageClass"
      storage_class = "COLDLINE"
    }
  }
}

resource "google_storage_bucket_iam_member" "kore_cloud_run" {
  bucket = google_storage_bucket.kore.name
  role   = "roles/storage.objectAdmin"
  member = "serviceAccount:${google_service_account.kore_cloud_run.email}"
}

# ==================== KMS for Encryption ====================
resource "google_kms_key_ring" "kore" {
  count    = var.environment == "prod" ? 1 : 0
  name     = "kore-keyring-${var.environment}"
  location = var.gcp_region
}

resource "google_kms_crypto_key" "storage" {
  count           = var.environment == "prod" ? 1 : 0
  name            = "kore-storage-key"
  key_ring        = google_kms_key_ring.kore[0].id
  rotation_period = "7776000s"
  version_template {
    algorithm = "GOOGLE_SYMMETRIC_ENCRYPTION"
  }
}

# ==================== Cloud Run ====================
resource "google_cloud_run_service" "kore_cloud" {
  name     = "kore-cloud-${var.environment}"
  location = var.gcp_region

  template {
    spec {
      service_account_name = google_service_account.kore_cloud_run.email
      containers {
        image = "${var.gcp_region}-docker.pkg.dev/${var.gcp_project_id}/kore/kore-cloud:latest"
        ports {
          container_port = 8000
        }
        env {
          name  = "DATABASE_URL"
          value = "postgresql://koremaster:${var.db_password}@${google_sql_database_instance.kore.private_ip_address}:5432/kore"
        }
        env {
          name  = "STORAGE_BACKEND"
          value = "gcp"
        }
        env {
          name  = "GCP_BUCKET_NAME"
          value = google_storage_bucket.kore.name
        }
        env {
          name  = "RUST_LOG"
          value = var.log_level
        }
        resources {
          limits = {
            cpu    = "2"
            memory = "4Gi"
          }
        }
      }
      timeout_seconds = 3600
    }
    metadata {
      annotations = {
        "cloudsql-instances" = google_sql_database_instance.kore.connection_name
        "run.googleapis.com/cloudsql-instances" = google_sql_database_instance.kore.connection_name
      }
    }
  }

  traffic {
    percent         = 100
    latest_revision = true
  }

  depends_on = [google_project_service.run]
}

# ==================== Cloud Run IAM ====================
resource "google_cloud_run_service_iam_member" "public" {
  service  = google_cloud_run_service.kore_cloud.name
  location = google_cloud_run_service.kore_cloud.location
  role     = "roles/run.invoker"
  member   = "allUsers"
}

# ==================== Service Account ====================
resource "google_service_account" "kore_cloud_run" {
  account_id   = "kore-cloud-run-${var.environment}"
  display_name = "Kore Cloud Run Service Account"
}

resource "google_project_iam_member" "cloud_sql_client" {
  project = var.gcp_project_id
  role    = "roles/cloudsql.client"
  member  = "serviceAccount:${google_service_account.kore_cloud_run.email}"
}

resource "google_project_iam_member" "storage_admin" {
  project = var.gcp_project_id
  role    = "roles/storage.admin"
  member  = "serviceAccount:${google_service_account.kore_cloud_run.email}"
}

# ==================== Cloud Load Balancer ====================
resource "google_compute_backend_service" "kore" {
  name            = "kore-backend-${var.environment}"
  load_balancing_scheme = "EXTERNAL"

  custom_request_headers {
    headers = ["X-Client-Region:{client_region}"]
  }

  log_config {
    enable = true
  }
}

resource "google_compute_url_map" "kore" {
  name            = "kore-url-map-${var.environment}"
  default_service = google_compute_backend_service.kore.id

  host_rule {
    hosts        = ["kore-cloud-${var.environment}.${var.gcp_domain}"]
    path_matcher = "kore-paths"
  }

  path_matcher {
    name            = "kore-paths"
    default_service = google_compute_backend_service.kore.id
    path_rule {
      paths   = ["/health", "/status"]
      service = google_compute_backend_service.kore.id
    }
  }
}

resource "google_compute_ssl_certificate" "kore" {
  name            = "kore-cert-${var.environment}"
  certificate     = file("${path.module}/certs/certificate.crt")
  private_key     = file("${path.module}/certs/private.key")
  lifecycle {
    create_before_destroy = true
  }
}

resource "google_compute_target_https_proxy" "kore" {
  name             = "kore-https-proxy-${var.environment}"
  url_map          = google_compute_url_map.kore.id
  ssl_certificates = [google_compute_ssl_certificate.kore.id]
}

resource "google_compute_global_forwarding_rule" "kore" {
  name                  = "kore-forwarding-rule-${var.environment}"
  ip_protocol           = "TCP"
  load_balancing_scheme = "EXTERNAL"
  port_range            = "443"
  target                = google_compute_target_https_proxy.kore.id
}

# ==================== Monitoring ====================
resource "google_monitoring_notification_channel" "kore_email" {
  display_name = "Kore Alert - Email"
  type         = "email"
  labels = {
    email_address = var.alert_email
  }
}

resource "google_monitoring_alert_policy" "kore_cpu" {
  display_name = "Kore Cloud Run - High CPU"
  combiner     = "OR"
  enabled      = true

  conditions {
    display_name = "CPU utilization above 80%"
    condition_threshold {
      filter          = "resource.type=\"cloud_run_revision\" AND metric.type=\"run.googleapis.com/request_count\""
      duration        = "300s"
      comparison      = "COMPARISON_GT"
      threshold_value = 0.8
    }
  }

  notification_channels = [google_monitoring_notification_channel.kore_email.id]
}

# ==================== APIs ====================
resource "google_project_service" "run" {
  service = "run.googleapis.com"
  disable_on_destroy = false
}

resource "google_project_service" "sqladmin" {
  service = "sqladmin.googleapis.com"
  disable_on_destroy = false
}

resource "google_project_service" "storage" {
  service = "storage.googleapis.com"
  disable_on_destroy = false
}

resource "google_project_service" "cloudkms" {
  service = "cloudkms.googleapis.com"
  disable_on_destroy = false
}

# ==================== Outputs ====================
output "cloud_run_url" {
  value       = google_cloud_run_service.kore_cloud.status[0].url
  description = "Cloud Run service URL"
}

output "cloud_sql_connection_name" {
  value       = google_sql_database_instance.kore.connection_name
  description = "Cloud SQL connection name"
}

output "storage_bucket_name" {
  value       = google_storage_bucket.kore.name
  description = "Cloud Storage bucket name"
}

output "cloud_run_service_account" {
  value       = google_service_account.kore_cloud_run.email
  description = "Cloud Run service account email"
}
