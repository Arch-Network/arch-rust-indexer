provider "aws" {
  region = var.region
}
# IAM roles for ECS
data "aws_iam_policy_document" "ecs_task_assume" {
  statement {
    actions = ["sts:AssumeRole"]
    principals {
      type        = "Service"
      identifiers = ["ecs-tasks.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "ecs_execution" {
  name               = "arch-indexer-ecs-execution"
  assume_role_policy = data.aws_iam_policy_document.ecs_task_assume.json
}

resource "aws_iam_role_policy_attachment" "ecs_execution_policy" {
  role       = aws_iam_role.ecs_execution.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

resource "aws_iam_role" "ecs_task" {
  name               = "arch-indexer-ecs-task"
  assume_role_policy = data.aws_iam_policy_document.ecs_task_assume.json
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

  # Use default parameter group for simplicity
  # parameter_group_name = aws_db_parameter_group.postgres.name

  backup_retention_period = 7
  skip_final_snapshot    = true

  lifecycle {
    ignore_changes = [
      password,
    ]
  }
}

# Create Redis instance (equivalent to Memorystore)
resource "aws_elasticache_cluster" "redis" {
  cluster_id           = "arch-indexer-cache"
  engine              = "redis"
  engine_version      = "7.1"
  node_type           = "cache.t3.micro"
  num_cache_nodes     = 1
  parameter_group_name = "default.redis7"
  port                = 6379
  security_group_ids  = [aws_security_group.redis.id]
  subnet_group_name   = aws_elasticache_subnet_group.redis.name
}

# Create ECS Fargate service (equivalent to Cloud Run)
resource "aws_ecs_cluster" "main" {
  name = "arch-rust-indexer"
}

# CloudWatch log groups for ECS tasks
resource "aws_cloudwatch_log_group" "api" {
  name              = "/ecs/arch-indexer-api"
  retention_in_days = 14
}

resource "aws_cloudwatch_log_group" "frontend" {
  name              = "/ecs/arch-indexer-frontend"
  retention_in_days = 14
}

resource "aws_cloudwatch_log_group" "indexer" {
  name              = "/ecs/arch-indexer-indexer"
  retention_in_days = 14
}

resource "aws_cloudwatch_log_group" "dbinit" {
  name              = "/ecs/arch-indexer-dbinit"
  retention_in_days = 3
}

# Fetch DB password from SSM Parameter Store (SecureString)
data "aws_ssm_parameter" "db_password" {
  name            = var.ssm_db_password_name
  with_decryption = true
}

resource "aws_ecs_task_definition" "api" {
  family                   = "arch-indexer-api"
  requires_compatibilities = ["FARGATE"]
  network_mode            = "awsvpc"
  cpu                     = "1024"
  memory                  = "2048"
  execution_role_arn      = aws_iam_role.ecs_execution.arn
  task_role_arn           = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name  = "api-server"
      image = var.api_image
      
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
        # DATABASE__PASSWORD comes from SSM via secrets below
        {
          name  = "DATABASE__HOST"
          value = aws_db_instance.postgres.address
        },
        {
          name  = "DATABASE__PORT"
          value = tostring(aws_db_instance.postgres.port)
        },
        {
          name  = "DATABASE__DATABASE_NAME"
          value = "archindexer"
        },
        {
          name  = "DATABASE__MAX_CONNECTIONS"
          value = "50"
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
        # Ensure nested config picks up arch_node.url
        { name = "ARCH_NODE__URL", value = var.arch_node_url },
        { name = "ARCH_NODE_WEBSOCKET_URL", value = var.arch_node_ws_url },
        { name = "ARCH_NODE__WEBSOCKET_URL", value = var.arch_node_ws_url },
        { name = "INDEXER_RUNTIME", value = "atlas" },
        { name = "METRICS_ADDR", value = "0.0.0.0:${var.metrics_port}" },
        { name = "ATLAS_CHECKPOINT_BACKEND", value = var.atlas_checkpoint_backend },
        { name = "ARCH_MAX_CONCURRENCY", value = "256" },
        { name = "ARCH_BULK_BATCH_SIZE", value = "10000" },
        { name = "ARCH_FETCH_WINDOW_SIZE", value = "32768" },
        { name = "ARCH_INITIAL_BACKOFF_MS", value = "10" },
        { name = "ARCH_MAX_RETRIES", value = "5" },
        { name = "ATLAS_USE_COPY_BULK", value = "1" },
        # Back-compat single-underscore env if referenced elsewhere
        { name = "ARCH_NODE_URL", value = var.arch_node_url },
        # Apply DB TIMESTAMPTZ fix on startup (idempotent). AWS-only, does not affect local.
        { name = "APPLY_TS_TZ_FIX", value = "1" },
        # Ensure created_at index exists in prod clusters during rollout
        { name = "APPLY_CREATED_AT_INDEX", value = "1" },
        { name = "REDIS_URL", value = "redis://${aws_elasticache_cluster.redis.cache_nodes[0].address}:${aws_elasticache_cluster.redis.cache_nodes[0].port}" },
        # Avoid embedding password in env
      ]

      secrets = [
        {
          name      = "DATABASE__PASSWORD"
          valueFrom = data.aws_ssm_parameter.db_password.arn
        }
      ]

      portMappings = [
        {
          containerPort = var.api_port
          hostPort      = var.api_port
          protocol      = "tcp"
        }
      ]

      healthCheck = {
        command     = ["CMD-SHELL", "curl -f http://localhost:${var.api_port}/health || curl -f http://localhost:${var.api_port}/ || exit 1"]
        interval    = 30
        timeout     = 5
        retries     = 3
        startPeriod = 60
      }

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = "/ecs/arch-indexer-api"
          awslogs-region        = var.region
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])
}

resource "aws_ecs_task_definition" "dbinit" {
  family                   = "arch-indexer-dbinit"
  requires_compatibilities = ["FARGATE"]
  network_mode            = "awsvpc"
  cpu                     = "256"
  memory                  = "512"
  execution_role_arn      = aws_iam_role.ecs_execution.arn
  task_role_arn           = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name  = "dbinit"
      image = var.db_init_image
      environment = [
        { name = "PGHOST",     value = aws_db_instance.postgres.address },
        { name = "PGUSER",     value = var.db_username },
        { name = "PGDATABASE", value = "archindexer" }
      ]
      secrets = [
        {
          name      = "PGPASSWORD"
          valueFrom = data.aws_ssm_parameter.db_password.arn
        }
      ]
      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = "/ecs/arch-indexer-dbinit"
          awslogs-region        = var.region
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])
}

# API service behind ALB
resource "aws_ecs_service" "api" {
  name            = "arch-indexer-api"
  cluster         = aws_ecs_cluster.main.id
  task_definition = aws_ecs_task_definition.api.arn
  desired_count   = 2
  launch_type     = "FARGATE"

  network_configuration {
    subnets         = aws_subnet.private[*].id
    security_groups = [aws_security_group.ecs.id]
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.api.arn
    container_name   = "api-server"
    container_port   = var.api_port
  }
}

# Indexer task (no ALB)
resource "aws_ecs_task_definition" "indexer" {
  family                   = "arch-indexer-indexer"
  requires_compatibilities = ["FARGATE"]
  network_mode            = "awsvpc"
  cpu                     = "2048"
  memory                  = "4096"
  execution_role_arn      = aws_iam_role.ecs_execution.arn
  task_role_arn           = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name  = "indexer"
      image = var.indexer_image

      environment = [
        // Provide both nested DATABASE__* (used by config crate) and PG* (fallbacks)
        { name = "DATABASE__USERNAME", value = var.db_username },
        { name = "DATABASE__HOST", value = aws_db_instance.postgres.address },
        { name = "DATABASE__PORT", value = tostring(aws_db_instance.postgres.port) },
        { name = "DATABASE__DATABASE_NAME", value = "archindexer" },
        // Provide a fully-composed DATABASE_URL as an additional fallback
        { name = "DATABASE_URL", value = "postgresql://${var.db_username}:${data.aws_ssm_parameter.db_password.value}@${aws_db_instance.postgres.address}:${aws_db_instance.postgres.port}/archindexer" },
        // PG* fallbacks consumed by Settings::load()
        { name = "PGHOST", value = aws_db_instance.postgres.address },
        { name = "PGUSER", value = var.db_username },
        { name = "PGDATABASE", value = "archindexer" },
        { name = "ARCH_NODE__URL", value = var.arch_node_url },
        { name = "ARCH_NODE_WEBSOCKET_URL", value = var.arch_node_ws_url },
        { name = "REDIS_URL", value = "redis://${aws_elasticache_cluster.redis.cache_nodes[0].address}:${aws_elasticache_cluster.redis.cache_nodes[0].port}" },
        { name = "INDEXER_RUNTIME", value = "atlas" },
        { name = "METRICS_ADDR", value = "0.0.0.0:${var.metrics_port}" },
        { name = "ATLAS_CHECKPOINT_BACKEND", value = var.atlas_checkpoint_backend },
        { name = "ARCH_MAX_CONCURRENCY", value = "256" },
        { name = "ARCH_BULK_BATCH_SIZE", value = "10000" },
        { name = "ARCH_FETCH_WINDOW_SIZE", value = "32768" },
        { name = "ARCH_INITIAL_BACKOFF_MS", value = "10" },
        { name = "ARCH_MAX_RETRIES", value = "5" },
        { name = "ATLAS_USE_COPY_BULK", value = "1" },
        # Align seeding behavior with docker-compose
        { name = "ARCH_BUILTIN_PROGRAMS", value = "0000000000000000000000000000000000000000000000000000000000000001,ComputeBudget111111111111111111111111111111,VoteProgram111111111111111111111,StakeProgram11111111111111111111,BpfLoader11111111111111111111111,NativeLoader11111111111111111111,AplToken111111111111111111111111" },
        { name = "ARCH_FAST_FORWARD_WINDOW", value = "0" },
        { name = "ARCH_BACKFILL_PREFIX_ON_START", value = "0" },
        { name = "ARCH_PREFIX_BACKFILL_BATCH", value = "500" },
        { name = "ARCH_HEAL_MISSING_ON_START", value = "0" },
        { name = "ARCH_HEAL_CHUNK_SIZE", value = "100000" },
        # Realtime ingestion via websocket
        { name = "INDEXER__ENABLE_REALTIME", value = "1" },
        { name = "WEBSOCKET__ENABLED", value = "1" },
        { name = "WEBSOCKET__RECONNECT_INTERVAL_SECONDS", value = "1" },
        { name = "WEBSOCKET__MAX_RECONNECT_ATTEMPTS", value = "0" }
      ]

      secrets = [
        {
          name      = "DATABASE__PASSWORD"
          valueFrom = data.aws_ssm_parameter.db_password.arn
        },
        // Also provide PGPASSWORD for PG* fallback path
        {
          name      = "PGPASSWORD"
          valueFrom = data.aws_ssm_parameter.db_password.arn
        }
      ]

      portMappings = [
        { containerPort = var.metrics_port, hostPort = var.metrics_port, protocol = "tcp" }
      ]

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = "/ecs/arch-indexer-indexer"
          awslogs-region        = var.region
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])
}

resource "aws_ecs_service" "indexer" {
  name            = "arch-indexer-indexer"
  cluster         = aws_ecs_cluster.main.id
  task_definition = aws_ecs_task_definition.indexer.arn
  desired_count   = 4
  launch_type     = "FARGATE"

  network_configuration {
    subnets         = aws_subnet.private[*].id
    security_groups = [aws_security_group.ecs.id]
  }
}

# Frontend behind ALB
resource "aws_ecs_task_definition" "frontend" {
  family                   = "arch-indexer-frontend"
  requires_compatibilities = ["FARGATE"]
  network_mode            = "awsvpc"
  cpu                     = "512"
  memory                  = "1024"
  execution_role_arn      = aws_iam_role.ecs_execution.arn
  task_role_arn           = aws_iam_role.ecs_task.arn

  container_definitions = jsonencode([
    {
      name  = "frontend"
      image = var.frontend_image
      portMappings = [
        { containerPort = var.frontend_port, hostPort = var.frontend_port, protocol = "tcp" }
      ]
      environment = [
        { name = "NEXT_PUBLIC_API_URL", value = "http://${aws_lb.main.dns_name}" },
        { name = "NEXT_PUBLIC_WS_URL",  value = "ws://${aws_lb.main.dns_name}/ws" },
        { name = "ROLLOUT_TIMESTAMP",   value = tostring(timestamp()) }
      ]
      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = "/ecs/arch-indexer-frontend"
          awslogs-region        = var.region
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])
}

resource "aws_ecs_service" "frontend" {
  name            = "arch-indexer-frontend"
  cluster         = aws_ecs_cluster.main.id
  task_definition = aws_ecs_task_definition.frontend.arn
  desired_count   = 2
  launch_type     = "FARGATE"

  network_configuration {
    subnets         = aws_subnet.public[*].id
    security_groups = [aws_security_group.ecs.id]
    assign_public_ip = true
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.frontend.arn
    container_name   = "frontend"
    container_port   = var.frontend_port
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