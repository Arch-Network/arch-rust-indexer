
data "aws_route53_zone" "this" {
  provider     = aws.dns
  name         = var.zone_name
  private_zone = false
}

resource "aws_acm_certificate" "this" {
  provider           = aws.app
  domain_name       = var.hostname
  validation_method = "DNS"

  tags = { Name = var.hostname }
}

resource "aws_route53_record" "validation" {
  provider = aws.dns
  for_each = {
    for dvo in aws_acm_certificate.this.domain_validation_options :
    dvo.domain_name => {
      name  = dvo.resource_record_name
      type  = dvo.resource_record_type
      value = dvo.resource_record_value
    }
  }
  zone_id = data.aws_route53_zone.this.zone_id
  name    = each.value.name
  type    = each.value.type
  ttl     = 60
  records = [each.value.value]
}

resource "aws_acm_certificate_validation" "this" {
  provider                = aws.app
  certificate_arn         = aws_acm_certificate.this.arn
  validation_record_fqdns = [for r in aws_route53_record.validation : r.fqdn]
}

resource "aws_route53_record" "alias" {
  provider = aws.dns
  zone_id = data.aws_route53_zone.this.zone_id
  name    = var.hostname
  type    = "A"
  alias {
    name                   = var.alb_dns_name
    zone_id                = var.alb_zone_id
    evaluate_target_health = true
  }
}
