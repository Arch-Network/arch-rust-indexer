# Arch Indexer Deployment Guide

## Prerequisites
- Terraform installed
- AWS CLI or GCP CLI configured
- Docker installed (for building custom images)

# Terraform Configuration

## Setup
1. Copy the example variables file:
   ```bash
   cp example.tfvars terraform.tfvars
   ```
2. Edit `terraform.tfvars` with your actual values
3. Run terraform:
   ```bash
   terraform init
   terraform apply
   ```

Note: Never commit `terraform.tfvars` or `.tfstate` files as they may contain sensitive information.

## Deployment Steps

### AWS Deployment
1. Navigate to deploy/aws/terraform
2. Initialize Terraform:
   ```bash
   terraform init
   ```
3. Create a terraform.tfvars file with your variables:
   ```hcl
   aws_region = "us-west-2"
   db_username = "your_username"
   db_password = "your_password"
   indexer_image = "your-registry/arch-indexer:latest"
   ```
4. Deploy:
   ```bash
   terraform apply
   ```

### GCP Deployment
1. Navigate to deploy/gcp/terraform
2. Initialize Terraform:
   ```bash
   terraform init
   ```
3. Create a terraform.tfvars file with your variables:
   ```hcl
   project_id = "your-project-id"
   region = "us-central1"
   db_username = "your_username"
   db_password = "your_password"
   indexer_image = "your-registry/arch-indexer:latest"
   ```
4. Deploy:
   ```bash
   terraform apply
   ```

## Post-Deployment Steps
1. Initialize the database schema:
   ```bash
   arch-indexer init-db
   ```
2. Configure your environment variables
3. Start the indexer service