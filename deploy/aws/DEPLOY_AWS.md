# Arch Indexer – AWS Deployment Guide (ECS Fargate + RDS + ElastiCache)

This guide covers building and deploying the Arch Indexer microservices (api-server, indexer, frontend) to AWS using ECR, ECS Fargate, RDS (PostgreSQL), ElastiCache (Redis), and an ALB managed via Terraform.

## Prerequisites
- AWS account with permissions for ECS, ECR, RDS, ElastiCache, VPC, CloudWatch Logs, and ELB
- Tools installed: awscli v2, terraform (>=1.5), docker (with buildx), jq, git
- AWS auth:
  - If SSO: `aws sso login --profile <your_profile>`
  - Verify: `aws sts get-caller-identity`

## Key paths and resources
- Terraform root: `rust/deploy/aws/terraform`
- Build/push script: `rust/arch-indexer-microservices/deploy_aws.sh`
- ECS services (Terraform): `arch-indexer-api`, `arch-indexer-indexer`, `arch-indexer-frontend`
- ALB name (Terraform): `arch-indexer-alb`

## Build and push images (api, indexer, frontend)
From `rust/arch-indexer-microservices`:
```bash
# Optional overrides
export ACCOUNT_ID=590184001652
export REGION=us-east-1
# Optional AWS profile (if using SSO)
# export AWS_PROFILE=default

./deploy_aws.sh
```
Notes:
- Builds linux/amd64 images via docker buildx and pushes tags `latest` and `$(git rev-parse --short HEAD)`.
- If you see an "exec format error" in ECS, ensure images are built for linux/amd64 (script already does this).

## Provision / update infrastructure with Terraform
From `rust/deploy/aws/terraform`:

1) Initialize (first run or after upgrades):
```bash
TF_VAR_region=us-east-1 terraform init -upgrade | cat
```

2) Apply (required variables):
- `db_username`, `db_password` (RDS admin)
- `arch_node_url` (e.g., http://<arch-node-host>:<port>)
- `api_image`, `indexer_image`, `frontend_image` (full ECR URIs including tags)

Example using latest tags:
```bash
TF_VAR_region=us-east-1 \
TF_VAR_db_username=arch \
TF_VAR_db_password='StrongPass_2025!' \
TF_VAR_arch_node_url='http://44.196.173.35:8081' \
TF_VAR_api_image='590184001652.dkr.ecr.us-east-1.amazonaws.com/arch-indexer/api-server:latest' \
TF_VAR_indexer_image='590184001652.dkr.ecr.us-east-1.amazonaws.com/arch-indexer/indexer:latest' \
TF_VAR_frontend_image='590184001652.dkr.ecr.us-east-1.amazonaws.com/arch-indexer/frontend:latest' \
terraform apply -auto-approve | cat
```

3) Get public URL (ALB DNS):
```bash
echo "http://$(terraform output -raw service_url)/"
```

## Verify services
- ECS service status:
```bash
aws ecs describe-services --cluster arch-rust-indexer \
  --services arch-indexer-api arch-indexer-indexer arch-indexer-frontend \
  --region us-east-1 \
  | jq -r '.services[] | [.serviceName,.desiredCount,.runningCount,.status] | @tsv' | cat
```
- Probe ALB:
```bash
ALB=$(terraform output -raw service_url)
# Frontend should be 200
curl -I http://$ALB/ | head -n 1
# API routes are under /api (health route may not exist)
curl -I http://$ALB/api/ | head -n 1
```
- CloudWatch logs:
  - API: `/ecs/arch-indexer-api`
  - Indexer: `/ecs/arch-indexer-indexer`
  - Frontend: `/ecs/arch-indexer-frontend`

Example to tail latest API logs:
```bash
LG=/ecs/arch-indexer-api
STREAM=$(aws logs describe-log-streams --log-group-name $LG --order-by LastEventTime --descending --limit 1 --region us-east-1 | jq -r '.logStreams[0].logStreamName // empty')
[ -n "$STREAM" ] && aws logs get-log-events --log-group-name $LG --log-stream-name "$STREAM" --limit 100 --region us-east-1 | jq -r '.events[] | .message' | tail -n 100 | cat
```

## Redeploy after code changes
1) Rebuild and push images:
```bash
cd rust/arch-indexer-microservices
./deploy_aws.sh
```
2) Update task definitions to new tags (example using current git short SHA):
```bash
cd ../deploy/aws/terraform
TAG=$(git -C ../../arch-indexer-microservices rev-parse --short HEAD)
TF_VAR_api_image="590184001652.dkr.ecr.us-east-1.amazonaws.com/arch-indexer/api-server:$TAG" \
TF_VAR_indexer_image="590184001652.dkr.ecr.us-east-1.amazonaws.com/arch-indexer/indexer:$TAG" \
TF_VAR_frontend_image="590184001652.dkr.ecr.us-east-1.amazonaws.com/arch-indexer/frontend:$TAG" \
TF_VAR_arch_node_url='http://44.196.173.35:8081' \
TF_VAR_db_username=arch TF_VAR_db_password='StrongPass_2025!' TF_VAR_region=us-east-1 \
terraform apply -auto-approve | cat
```
3) Optionally force new deployments:
```bash
aws ecs update-service --cluster arch-rust-indexer --service arch-indexer-api --force-new-deployment --region us-east-1 | cat
aws ecs update-service --cluster arch-rust-indexer --service arch-indexer-frontend --force-new-deployment --region us-east-1 | cat
```

## Purge / recreate the database
- Scale down services (recommended for destructive ops):
```bash
aws ecs update-service --cluster arch-rust-indexer --service arch-indexer-api --desired-count 0 --region us-east-1 | cat
aws ecs update-service --cluster arch-rust-indexer --service arch-indexer-indexer --desired-count 0 --region us-east-1 | cat
aws ecs update-service --cluster arch-rust-indexer --service arch-indexer-frontend --desired-count 0 --region us-east-1 | cat
```

- Purge using a one-off indexer task (RESET_DB):
```bash
TASK_DEF=arch-indexer-indexer:8   # adjust revision as needed
CLUSTER=arch-rust-indexer
SUB1=<private-subnet-a>   # e.g., subnet-074e6d6ae0458049e
SUB2=<private-subnet-b>   # e.g., subnet-0b65eda1c175dd453
SG=<ecs-security-group>   # e.g., sg-0d99204086a9702e0
aws ecs run-task \
  --cluster $CLUSTER \
  --launch-type FARGATE \
  --count 1 \
  --task-definition $TASK_DEF \
  --network-configuration "awsvpcConfiguration={subnets=[$SUB1,$SUB2],securityGroups=[$SG],assignPublicIp=DISABLED}" \
  --overrides '{"containerOverrides":[{"name":"indexer","environment":[{"name":"RESET_DB","value":"true"},{"name":"RESET_AND_EXIT","value":"true"}]}]}' \
  --region us-east-1 | cat
```

- Recreate RDS with Terraform (destructive):
```bash
# Destroy only RDS
TF_VAR_region=us-east-1 TF_VAR_db_username=arch TF_VAR_db_password='StrongPass_2025!' \
TF_VAR_arch_node_url='http://44.196.173.35:8081' \
TF_VAR_api_image='590184001652.dkr.ecr.us-east-1.amazonaws.com/arch-indexer/api-server:latest' \
TF_VAR_indexer_image='590184001652.dkr.ecr.us-east-1.amazonaws.com/arch-indexer/indexer:latest' \
TF_VAR_frontend_image='590184001652.dkr.ecr.us-east-1.amazonaws.com/arch-indexer/frontend:latest' \
terraform destroy -target=aws_db_instance.postgres -auto-approve | cat

# Recreate only RDS
terraform apply -target=aws_db_instance.postgres -auto-approve | cat
```

- Scale services back up:
```bash
aws ecs update-service --cluster arch-rust-indexer --service arch-indexer-api --desired-count 2 --region us-east-1 | cat
aws ecs update-service --cluster arch-rust-indexer --service arch-indexer-indexer --desired-count 1 --region us-east-1 | cat
aws ecs update-service --cluster arch-rust-indexer --service arch-indexer-frontend --desired-count 2 --region us-east-1 | cat
```

## Accessing the public URL
- Terraform:
```bash
echo "http://$(terraform output -raw service_url)/"
```
- AWS Console: EC2 → Load Balancers → select `arch-indexer-alb` → copy the DNS name

## Common issues & fixes
- RDS password rejected: avoid forbidden characters ('/', '@', '"', space)
- Redis parameter group mismatch: Terraform uses `engine_version = "7.1"` and `parameter_group_name = "default.redis7"`
- CloudWatch log group missing: Terraform creates `/ecs/arch-indexer-api`, `/ecs/arch-indexer-indexer`, `/ecs/arch-indexer-frontend`
- ECR auth/i-o timeout for frontend pulls: ensure frontend ECS service sets `assignPublicIp = true` and runs in public subnets with internet route
- Exec format error: images must be built for linux/amd64 (buildx)
- API reports missing `arch_node`: API now defaults `arch_node.url` and honors `ARCH_NODE__URL`/`ARCH_NODE_URL`
- ALB health checks: use `/` with matcher `200-399`; `/api/health` may not exist

## Useful inspection commands
- Show running task images for a service:
```bash
aws ecs list-tasks --cluster arch-rust-indexer --service-name arch-indexer-frontend --region us-east-1 \
| jq -r '.taskArns[]' \
| while read -r t; do 
  aws ecs describe-tasks --cluster arch-rust-indexer --tasks "$t" --region us-east-1 \
    | jq -r '.tasks[] | [.taskArn, .containers[0].image] | @tsv';
  done | cat
```
- Force new deployment:
```bash
aws ecs update-service --cluster arch-rust-indexer --service arch-indexer-api --force-new-deployment --region us-east-1 | cat
```

---
This guide reflects the current Terraform in `rust/deploy/aws/terraform` and the build script `rust/arch-indexer-microservices/deploy_aws.sh`. 
