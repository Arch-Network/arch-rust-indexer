variable "zone_name" {
  description = "Route53 hosted zone name (with trailing dot)"
  type        = string
}

variable "hostname" {
  description = "Full hostname (record) to manage"
  type        = string
}

variable "alb_dns_name" {
  description = "ALB DNS name to alias to"
  type        = string
}

variable "alb_zone_id" {
  description = "ALB canonical hosted zone id"
  type        = string
}
