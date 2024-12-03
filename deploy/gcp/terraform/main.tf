provider "google" {
  project = var.project_id
  region  = var.region
}

resource "google_sql_database_instance" "instance" {
  name             = "arch-rust-indexer"
  database_version = "POSTGRES_15"
  region           = var.region
  
  settings {
    tier = "db-custom-4-15360"
    
    database_flags {
      name  = "max_connections"
      value = "100"
    }
    
    database_flags {
      name  = "shared_buffers"
      value = "384000"
    }
    
    database_flags {
      name  = "effective_cache_size"
      value = "196608"
    }
    
    backup_configuration {
      enabled = true
    }
  }

  deletion_protection = false
}

resource "google_sql_database" "database" {
  name     = "archindexer"
  instance = google_sql_database_instance.instance.name
  
  depends_on = [google_sql_database_instance.instance]
}

resource "google_sql_user" "user" {
  name     = var.db_username
  instance = google_sql_database_instance.instance.name
  password = var.db_password
  
  depends_on = [google_sql_database_instance.instance]
}

# Enable required APIs
resource "google_project_service" "run" {
  service = "run.googleapis.com"
  disable_on_destroy = false
}

// Enable Redis API
resource "google_project_service" "redis" {
  service = "redis.googleapis.com"
  disable_on_destroy = false
}

// Create Redis instance
resource "google_redis_instance" "cache" {
  name           = "arch-indexer-cache"
  tier           = "STANDARD_HA"
  memory_size_gb = 5
  
  region = var.region
  
  depends_on = [google_project_service.redis]
}

# Cloud Run service
resource "google_cloud_run_service" "indexer" {
  name     = "arch-rust-indexer"
  location = var.region

  template {
    metadata {
          annotations = {
            "run.googleapis.com/cloudsql-instances" = google_sql_database_instance.instance.connection_name
            "run.googleapis.com/cpu-throttling"     = "false"
            "run.googleapis.com/execution-environment" = "gen2"
          }
        }
    
    spec {
      containers {
        image = "gcr.io/${var.project_id}/arch-rust-indexer:fe2ab9c"

        env {
          name  = "APPLICATION__CORS_ALLOW_ORIGIN"
          value = "*"  # Or "https://graffiti.arch.network" for production
        }
        env {
          name  = "APPLICATION__CORS_ALLOW_METHODS"
          value = "GET, POST, OPTIONS"
        }
        env {
          name  = "APPLICATION__CORS_ALLOW_HEADERS"
          value = "Content-Type, Authorization"
        }
        
        resources {
          limits = {
            cpu    = "4000m"     # 4 vCPUs
            memory = "8Gi"       # 8GB RAM
          }
          requests = {
            cpu    = "2000m"     # 2 vCPU minimum
            memory = "4Gi"       # 4GB RAM minimum
          }
        }

        # DatabaseSettings
        env {
          name  = "DATABASE__USERNAME"
          value = var.db_username
        }
        env {
          name  = "DATABASE__PASSWORD"
          value = var.db_password
        }
        env {
          name  = "DATABASE__HOST"
          value = "/cloudsql/${google_sql_database_instance.instance.connection_name}"
        }
        env {
          name  = "DATABASE__PORT"
          value = "5432"
        }
        env {
          name  = "DATABASE__DATABASE_NAME"
          value = "archindexer"
        }
        env {
          name  = "DATABASE__MAX_CONNECTIONS"
          value = "50"
        }
        env {
          name  = "DATABASE__MIN_CONNECTIONS"
          value = "10"
        }

        # ApplicationSettings
        env {
          name  = "APPLICATION__PORT"
          value = "8080"
        }
        env {
          name  = "APPLICATION__HOST"
          value = "0.0.0.0"
        }

        # ArchNodeSettings
        env {
          name  = "ARCH_NODE__URL"
          value = var.arch_node_url
        }

        # RedisSettings
        env {
          name  = "REDIS__URL"
          value = "redis://${google_redis_instance.cache.host}:${google_redis_instance.cache.port}"
        }

        # IndexerSettings
        env {
          name  = "INDEXER__BATCH_SIZE"
          value = "1000"
        }
        env {
          name  = "INDEXER__CONCURRENT_BATCHES"
          value = "20"
        }

        ports {
          container_port = 8080
        }

        startup_probe {
          http_get {
            path = "/"
          }
          initial_delay_seconds = 10
          timeout_seconds = 3
          period_seconds = 5
          failure_threshold = 12  # Allow up to 1 minute for startup
        }

        liveness_probe {
          http_get {
            path = "/"
          }
          initial_delay_seconds = 15
          timeout_seconds = 3
          period_seconds = 30
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