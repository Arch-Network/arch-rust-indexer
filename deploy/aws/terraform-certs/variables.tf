variable "app_region" {
  description = "AWS region of the ALB/application account"
  type        = string
  default     = "us-east-1"
}

variable "dns_region" {
  description = "AWS region to use for the DNS account (any, Route53 is global)"
  type        = string
  default     = "us-east-1"
}

variable "app_profile" {
  description = "Named AWS profile for the ALB/application account (SSO/profile)"
  type        = string
  default     = ""
}

variable "dns_profile" {
  description = "Named AWS profile for the DNS account (SSO/profile)"
  type        = string
  default     = ""
}

variable "zone_name" {
  description = "Hosted zone name with trailing dot"
  type        = string
  default     = "test.arch.network."
}

variable "hostname" {
  description = "Full hostname (record) to manage"
  type        = string
}

variable "alb_name" {
  description = "Existing ALB name to alias (must already exist)"
  type        = string
}

variable "dns_assume_role_arn" {
  description = "ARN of role to assume for DNS operations (if cross-account)"
  type        = string
  default     = ""
}
