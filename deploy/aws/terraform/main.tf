provider "aws" {
  region = var.aws_region
}

module "vpc" {
  source = "terraform-aws-modules/vpc/aws"
  
  name = "arch-indexer-vpc"
  cidr = "10.0.0.0/16"
  
  azs             = ["${var.aws_region}a", "${var.aws_region}b"]
  private_subnets = ["10.0.1.0/24", "10.0.2.0/24"]
  public_subnets  = ["10.0.101.0/24", "10.0.102.0/24"]
  
  enable_nat_gateway = true
}

module "rds" {
  source = "terraform-aws-modules/rds/aws"
  
  identifier = "arch-indexer-db"
  
  engine               = "postgres"
  engine_version       = "13.7"
  family              = "postgres13"
  major_engine_version = "13"
  instance_class       = "db.t3.medium"
  
  allocated_storage     = 20
  max_allocated_storage = 100
  
  db_name  = "archindexer"
  username = var.db_username
  password = var.db_password
  port     = 5432
  
  vpc_security_group_ids = [aws_security_group.rds.id]
  subnet_ids             = module.vpc.private_subnets
}

module "elasticache" {
  source = "terraform-aws-modules/elasticache/aws"
  
  cluster_id           = "arch-indexer-redis"
  engine              = "redis"
  engine_version      = "6.x"
  node_type           = "cache.t3.micro"
  num_cache_nodes     = 1
  
  subnet_ids          = module.vpc.private_subnets
  security_group_ids  = [aws_security_group.redis.id]
}

resource "aws_ecs_cluster" "main" {
  name = "arch-indexer-cluster"
}

resource "aws_ecs_task_definition" "indexer" {
  family                   = "arch-indexer"
  requires_compatibilities = ["FARGATE"]
  network_mode            = "awsvpc"
  cpu                     = 1024
  memory                  = 2048
  
  container_definitions = jsonencode([
    {
      name  = "arch-indexer"
      image = var.indexer_image
      environment = [
        { name = "DATABASE__USERNAME", value = var.db_username },
        { name = "DATABASE__PASSWORD", value = var.db_password },
        { name = "DATABASE__HOST", value = module.rds.db_instance_endpoint },
        { name = "REDIS__URL", value = "redis://${module.elasticache.cluster_endpoint}" }
      ]
    }
  ])
}