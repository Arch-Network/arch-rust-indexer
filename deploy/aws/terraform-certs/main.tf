terraform {
  required_version = ">= 1.3.0"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = ">= 5.0"
    }
  }
}

provider "aws" {
  alias  = "app"
  region = var.app_region
  profile = var.app_profile != "" ? var.app_profile : null
}

provider "aws" {
  alias  = "dns"
  region = var.dns_region
  profile = var.dns_profile != "" ? var.dns_profile : null
  
  dynamic "assume_role" {
    for_each = var.dns_assume_role_arn != "" ? [1] : []
    content {
      role_arn = var.dns_assume_role_arn
    }
  }
}

data "aws_lb" "existing" {
  provider = aws.app
  name = var.alb_name
}

module "cert_dns" {
  source       = "../terraform/modules/cert-dns"
  providers = {
    aws.dns = aws.dns
    aws.app = aws.app
  }
  zone_name    = var.zone_name
  hostname     = var.hostname
  alb_dns_name = data.aws_lb.existing.dns_name
  alb_zone_id  = data.aws_lb.existing.zone_id
}

output "certificate_arn" {
  value = module.cert_dns.certificate_arn
}
