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