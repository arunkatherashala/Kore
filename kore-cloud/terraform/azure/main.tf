terraform {
  required_version = ">= 1.0"
  required_providers {
    azurerm = {
      source  = "hashicorp/azurerm"
      version = "~> 3.0"
    }
  }
}

provider "azurerm" {
  features {}
  subscription_id = var.azure_subscription_id
}

# ==================== Resource Group ====================
resource "azurerm_resource_group" "kore" {
  name     = "rg-kore-${var.environment}"
  location = var.azure_region

  tags = {
    Environment = var.environment
    Project     = "Kore"
  }
}

# ==================== Azure Database for PostgreSQL ====================
resource "azurerm_postgresql_flexible_server" "kore" {
  name                   = "kore-postgres-${var.environment}"
  resource_group_name    = azurerm_resource_group.kore.name
  location               = azurerm_resource_group.kore.location
  administrator_login    = "koremaster"
  administrator_password = var.db_password
  version                = "15"
  storage_mb             = 32768
  sku_name               = "B_Standard_B2s"

  backup_retention_days        = 30
  geo_redundant_backup_enabled = var.environment == "prod"
  auto_grow_enabled            = true

  tags = {
    Environment = var.environment
    Project     = "Kore"
  }
}

resource "azurerm_postgresql_flexible_server_firewall_rule" "kore" {
  name             = "AllowAzureServices"
  server_id        = azurerm_postgresql_flexible_server.kore.id
  start_ip_address = "0.0.0.0"
  end_ip_address   = "0.0.0.0"
}

# ==================== Azure Blob Storage ====================
resource "azurerm_storage_account" "kore" {
  name                     = "korestg${var.environment}${substr(var.azure_subscription_id, 0, 8)}"
  resource_group_name      = azurerm_resource_group.kore.name
  location                 = azurerm_resource_group.kore.location
  account_tier             = "Standard"
  account_replication_type = var.environment == "prod" ? "GRS" : "LRS"

  https_traffic_only_enabled = true
  min_tls_version            = "TLS1_2"

  tags = {
    Environment = var.environment
    Project     = "Kore"
  }
}

resource "azurerm_storage_container" "kore" {
  name                  = "kore-files"
  storage_account_name  = azurerm_storage_account.kore.name
  container_access_type = "private"
}

# ==================== Azure Container Registry ====================
resource "azurerm_container_registry" "kore" {
  name                = "korereg${var.environment}${substr(var.azure_subscription_id, 0, 8)}"
  resource_group_name = azurerm_resource_group.kore.name
  location            = azurerm_resource_group.kore.location
  sku                 = var.environment == "prod" ? "Premium" : "Standard"
  admin_enabled       = true

  tags = {
    Environment = var.environment
    Project     = "Kore"
  }
}

# ==================== Azure Container Instance ====================
resource "azurerm_container_group" "kore_cloud" {
  name                = "kore-cloud-${var.environment}"
  location            = azurerm_resource_group.kore.location
  resource_group_name = azurerm_resource_group.kore.name
  ip_address_type     = "Public"
  dns_name_label      = "kore-cloud-${var.environment}"
  os_type             = "Linux"

  container {
    name   = "kore-cloud"
    image  = "${azurerm_container_registry.kore.login_server}/kore-cloud:latest"
    cpu    = "1.0"
    memory = "1.5"
    port {
      port     = 8000
      protocol = "TCP"
    }
    environment_variables = {
      DATABASE_URL = "postgresql://koremaster:${var.db_password}@${azurerm_postgresql_flexible_server.kore.fqdn}:5432/kore"
      STORAGE_BACKEND = "azure"
      AZURE_STORAGE_ACCOUNT = azurerm_storage_account.kore.name
      AZURE_STORAGE_CONTAINER = azurerm_storage_container.kore.name
      RUST_LOG = var.log_level
    }
    secure_environment_variables = {
      AZURE_STORAGE_KEY = azurerm_storage_account.kore.primary_access_key
    }
  }

  image_registry_credential {
    server   = azurerm_container_registry.kore.login_server
    username = azurerm_container_registry.kore.admin_username
    password = azurerm_container_registry.kore.admin_password
  }

  tags = {
    Environment = var.environment
    Project     = "Kore"
  }
}

# ==================== Azure Application Insights ====================
resource "azurerm_application_insights" "kore" {
  name                = "kore-insights-${var.environment}"
  location            = azurerm_resource_group.kore.location
  resource_group_name = azurerm_resource_group.kore.name
  application_type    = "web"

  tags = {
    Environment = var.environment
    Project     = "Kore"
  }
}

# ==================== Azure Key Vault ====================
resource "azurerm_key_vault" "kore" {
  name                = "kore-vault-${var.environment}"
  location            = azurerm_resource_group.kore.location
  resource_group_name = azurerm_resource_group.kore.name
  tenant_id           = data.azurerm_client_config.current.tenant_id
  sku_name            = "standard"

  access_policy {
    tenant_id = data.azurerm_client_config.current.tenant_id
    object_id = data.azurerm_client_config.current.object_id

    secret_permissions = [
      "Get",
      "Set",
      "List",
      "Delete",
    ]
  }

  tags = {
    Environment = var.environment
    Project     = "Kore"
  }
}

resource "azurerm_key_vault_secret" "db_password" {
  name         = "db-password"
  value        = var.db_password
  key_vault_id = azurerm_key_vault.kore.id
}

resource "azurerm_key_vault_secret" "storage_key" {
  name         = "storage-key"
  value        = azurerm_storage_account.kore.primary_access_key
  key_vault_id = azurerm_key_vault.kore.id
}

# ==================== Data Source ====================
data "azurerm_client_config" "current" {}

# ==================== Outputs ====================
output "container_group_fqdn" {
  value       = azurerm_container_group.kore_cloud.fqdn
  description = "FQDN of the container group"
}

output "postgres_fqdn" {
  value       = azurerm_postgresql_flexible_server.kore.fqdn
  description = "PostgreSQL server FQDN"
}

output "storage_account_name" {
  value       = azurerm_storage_account.kore.name
  description = "Storage account name"
}

output "container_registry_login_server" {
  value       = azurerm_container_registry.kore.login_server
  description = "Container registry login server"
}

output "key_vault_id" {
  value       = azurerm_key_vault.kore.id
  description = "Key Vault ID"
}
