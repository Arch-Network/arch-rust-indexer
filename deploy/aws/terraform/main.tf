provider "aws" {
  region = var.region
}

# Enable required APIs/services
resource "aws_vpc" "main" {
  cidr_block           = "10.0.0.0/16"
  enable_dns_hostnames = true
  enable_dns_support   = true

  tags = {
    Name = "arch-indexer-vpc"
  }
}

# Create subnets
resource "aws_subnet" "private" {
  count             = 2
  vpc_id            = aws_vpc.main.id
  cidr_block        = "10.0.${count.index + 1}.0/24"
  availability_zone = "${var.region}${count.index == 0 ? "a" : "b"}"

  tags = {
    Name = "arch-indexer-private-${count.index + 1}"
  }
}

resource "aws_subnet" "public" {
  count             = 2
  vpc_id            = aws_vpc.main.id
  cidr_block        = "10.0.${count.index + 101}.0/24"
  availability_zone = "${var.region}${count.index == 0 ? "a" : "b"}"

  tags = {
    Name = "arch-indexer-public-${count.index + 1}"
  }
}

# Create RDS instance (equivalent to Cloud SQL)
resource "aws_db_instance" "postgres" {
  identifier        = "arch-rust-indexer"
  engine           = "postgres"
  engine_version   = "15"
  instance_class   = "db.t3.medium"
  allocated_storage = 20

  db_name  = "archindexer"
  username = var.db_username
  password = var.db_password

  vpc_security_group_ids = [aws_security_group.postgres.id]
  db_subnet_group_name   = aws_db_subnet_group.postgres.name

  parameter_group_name = aws_db_parameter_group.postgres.name

  backup_retention_period = 7
  skip_final_snapshot    = true
}

# Create Redis instance (equivalent to Memorystore)
resource "aws_elasticache_cluster" "redis" {
  cluster_id           = "arch-indexer-cache"
  engine              = "redis"
  node_type           = "cache.t3.micro"
  num_cache_nodes     = 1
  parameter_group_name = "default.redis6.x"
  port                = 6379
  security_group_ids  = [aws_security_group.redis.id]
  subnet_group_name   = aws_elasticache_subnet_group.redis.name
}

# Create ECS Fargate service (equivalent to Cloud Run)
resource "aws_ecs_cluster" "main" {
  name = "arch-rust-indexer"
}

resource "aws_ecs_task_definition" "indexer" {
  family                   = "arch-rust-indexer"
  requires_compatibilities = ["FARGATE"]
  network_mode            = "awsvpc"
  cpu                     = "2048"
  memory                  = "4096"
  execution_role_arn      = aws_iam_role.ecs_execution.arn
  task_role_arn           = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name  = "arch-rust-indexer"
      image = "public.ecr.aws/your-repo/arch-rust-indexer:latest"
      
      environment = [
        {
          name  = "APPLICATION__CORS_ALLOW_ORIGIN"
          value = "*"
        },
        {
          name  = "APPLICATION__CORS_ALLOW_METHODS"
          value = "GET, POST, OPTIONS"
        },
        {
          name  = "APPLICATION__CORS_ALLOW_HEADERS"
          value = "Content-Type, Authorization"
        },
        {
          name  = "DATABASE__USERNAME"
          value = var.db_username
        },
        {
          name  = "DATABASE__PASSWORD"
          value = var.db_password
        },
        {
          name  = "DATABASE__HOST"
          value = aws_db_instance.postgres.endpoint
        },
        {
          name  = "DATABASE__PORT"
          value = "5432"
        },
        {
          name  = "DATABASE__DATABASE_NAME"
          value = "archindexer"
        },
        {
          name  = "DATABASE__MAX_CONNECTIONS"
          value = "30"
        },
        {
          name  = "DATABASE__MIN_CONNECTIONS"
          value = "5"
        },
        {
          name  = "APPLICATION__PORT"
          value = "8080"
        },
        {
          name  = "APPLICATION__HOST"
          value = "0.0.0.0"
        },
        {
          name  = "ARCH_NODE__URL"
          value = var.arch_node_url
        },
        {
          name  = "REDIS__URL"
          value = "redis://${aws_elasticache_cluster.redis.cache_nodes[0].address}:${aws_elasticache_cluster.redis.cache_nodes[0].port}"
        },
        {
          name  = "INDEXER__BATCH_SIZE"
          value = "2000"
        },
        {
          name  = "INDEXER__CONCURRENT_BATCHES"
          value = "40"
        }
      ]

      portMappings = [
        {
          containerPort = 8080
          hostPort      = 8080
          protocol      = "tcp"
        }
      ]

      healthCheck = {
        command     = ["CMD-SHELL", "curl -f http://localhost:8080/ || exit 1"]
        interval    = 30
        timeout     = 5
        retries     = 3
        startPeriod = 60
      }

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = "/ecs/arch-rust-indexer"
          awslogs-region        = var.region
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])
}

# Create ECS service
resource "aws_ecs_service" "indexer" {
  name            = "arch-rust-indexer"
  cluster         = aws_ecs_cluster.main.id
  task_definition = aws_ecs_task_definition.indexer.arn
  desired_count   = 1
  launch_type     = "FARGATE"

  network_configuration {
    subnets         = aws_subnet.private[*].id
    security_groups = [aws_security_group.ecs.id]
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.indexer.arn
    container_name   = "arch-rust-indexer"
    container_port   = 8080
  }
}

# Add outputs
output "database_endpoint" {
  value = aws_db_instance.postgres.endpoint
}

output "redis_endpoint" {
  value = "${aws_elasticache_cluster.redis.cache_nodes[0].address}:${aws_elasticache_cluster.redis.cache_nodes[0].port}"
}

output "service_url" {
  value = aws_lb.main.dns_name
}