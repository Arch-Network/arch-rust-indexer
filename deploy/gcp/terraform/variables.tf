variable "project_id" {
  description = "The GCP project ID"
  type        = string
}

variable "region" {
  description = "The GCP region"
  type        = string
  default     = "us-central1"
}

variable "db_username" {
  description = "Database user name"
  type        = string
}

variable "db_password" {
  description = "Database password"
  type        = string
  sensitive   = true
}

variable "arch_node_url" {
  description = "URL of the Arch Node RPC endpoint"
  type        = string
  default     = "http://leader:9002"  # Default value, should be overridden
}