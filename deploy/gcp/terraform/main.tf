provider "google" {
  project = var.project_id
  region  = var.region
}

resource "google_sql_database_instance" "instance" {
  name             = "arch-rust-indexer"
  database_version = "POSTGRES_15"
  region           = var.region
  
  settings {
    tier = "db-f1-micro"
    
    backup_configuration {
      enabled = true
    }
  }

  deletion_protection = false
}

resource "google_sql_database" "database" {
  name     = "archindexer"
  instance = google_sql_database_instance.instance.name
}

resource "google_sql_user" "user" {
  name     = var.db_username
  instance = google_sql_database_instance.instance.name
  password = var.db_password
}

# Enable required APIs
resource "google_project_service" "run" {
  service = "run.googleapis.com"
  disable_on_destroy = false
}

# Cloud Run service
resource "google_cloud_run_service" "indexer" {
  name     = "arch-rust-indexer"
  location = var.region

  template {
    spec {
      containers {
        image = "gcr.io/${var.project_id}/arch-indexer:latest"
        
        env {
          name  = "DATABASE_URL"
          value = "postgresql://${var.db_username}:${var.db_password}@${google_sql_database_instance.instance.connection_name}/archindexer"
        }
        
        env {
          name  = "RUST_LOG"
          value = "info"
        }

        env {
          name  = "ARCH_NODE_URL"
          value = var.arch_node_url
        }

        env {
          name  = "APPLICATION__PORT"
          value = "8080"
        }

        env {
          name  = "APPLICATION__HOST"
          value = "0.0.0.0"
        }

        ports {
          container_port = 8080
        }
      }
    }
  }

  depends_on = [google_project_service.run]
}

# Make the service public
resource "google_cloud_run_service_iam_member" "public" {
  service  = google_cloud_run_service.indexer.name
  location = google_cloud_run_service.indexer.location
  role     = "roles/run.invoker"
  member   = "allUsers"
}

# Add service URL to outputs
output "service_url" {
  value = google_cloud_run_service.indexer.status[0].url
}