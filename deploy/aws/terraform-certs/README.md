Usage: Cross-account DNS + ACM for ALB

Prereqs:
- Your shell must be able to assume two roles/profiles:
  - APP account (ALB lives here)
  - DNS account (Route53 hosted zone lives here)

Example env (Mac zsh):
```
export AWS_PROFILE=dns-account
export TF_VAR_alb_name=arch-indexer-alb
export TF_VAR_hostname=explorer-beta.test.arch.network
```

If you use named profiles, set them when running terraform:
```
AWS_PROFILE=dns-account terraform -chdir=. init
AWS_PROFILE=dns-account terraform -chdir=. plan \
  -var app_region=us-east-1 \
  -var dns_region=us-east-1 \
  -var zone_name=test.arch.network. \
  -var hostname=explorer-beta.test.arch.network \
  -var alb_name=arch-indexer-alb
```

This stack:
- Looks up the ALB in the APP account (via provider aws.app)
- Creates/validates an ACM certificate in the DNS account
- Creates the Route53 alias record to the ALB

After apply, copy the output certificate_arn into the main stack variable https_certificate_arn and apply that stack to attach the cert to the ALB listener.
