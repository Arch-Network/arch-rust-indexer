variable "region" {
  description = "AWS region"
  type        = string
  default     = "us-west-2"
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
  default     = "http://leader:9002"
}

# ECR image URIs (including tag) for each service
variable "api_image" {
  description = "ECR image for api-server"
  type        = string
}

variable "indexer_image" {
  description = "ECR image for indexer"
  type        = string
}

variable "frontend_image" {
  description = "ECR image for frontend"
  type        = string
}

variable "api_port" {
  description = "Container port for api-server"
  type        = number
  default     = 8080
}

variable "frontend_port" {
  description = "Container port for frontend"
  type        = number
  default     = 3000
}
