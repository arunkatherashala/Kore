terraform {
  required_version = ">= 1.0"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

provider "aws" {
  region = var.aws_region
}

# ==================== RDS PostgreSQL ====================
resource "aws_rds_cluster" "kore_postgres" {
  cluster_identifier      = "kore-postgres-${var.environment}"
  engine                  = "aurora-postgresql"
  engine_version          = "15.2"
  database_name           = "kore"
  master_username         = "koremaster"
  master_password         = var.db_password
  db_subnet_group_name    = aws_db_subnet_group.kore.name
  vpc_security_group_ids  = [aws_security_group.rds.id]
  backup_retention_period = 30
  preferred_backup_window = "03:00-04:00"
  storage_encrypted       = true
  skip_final_snapshot     = var.environment != "prod"

  tags = {
    Name        = "kore-postgres-${var.environment}"
    Environment = var.environment
  }
}

resource "aws_db_subnet_group" "kore" {
  name       = "kore-subnet-group"
  subnet_ids = var.database_subnets

  tags = {
    Name = "kore-subnet-group"
  }
}

# ==================== S3 Bucket ====================
resource "aws_s3_bucket" "kore_storage" {
  bucket = "kore-storage-${var.aws_account_id}-${var.environment}"

  tags = {
    Name        = "kore-storage-${var.environment}"
    Environment = var.environment
  }
}

resource "aws_s3_bucket_versioning" "kore_storage" {
  bucket = aws_s3_bucket.kore_storage.id

  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "kore_storage" {
  bucket = aws_s3_bucket.kore_storage.id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_bucket_public_access_block" "kore_storage" {
  bucket = aws_s3_bucket.kore_storage.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

# ==================== ECS Cluster ====================
resource "aws_ecs_cluster" "kore" {
  name = "kore-cluster-${var.environment}"

  setting {
    name  = "containerInsights"
    value = "enabled"
  }

  tags = {
    Name        = "kore-cluster-${var.environment}"
    Environment = var.environment
  }
}

# ==================== Task Definition ====================
resource "aws_ecs_task_definition" "kore_cloud" {
  family                   = "kore-cloud-${var.environment}"
  network_mode             = "awsvpc"
  requires_compatibilities = ["FARGATE"]
  cpu                      = "1024"
  memory                   = "2048"
  execution_role_arn       = aws_iam_role.ecs_task_execution_role.arn
  task_role_arn            = aws_iam_role.ecs_task_role.arn

  container_definitions = jsonencode([{
    name      = "kore-cloud"
    image     = "${var.ecr_repository_url}:${var.image_tag}"
    essential = true
    portMappings = [{
      containerPort = 8000
      hostPort      = 8000
      protocol      = "tcp"
    }]
    environment = [
      {
        name  = "DATABASE_URL"
        value = "postgresql://${aws_rds_cluster.kore_postgres.master_username}:${var.db_password}@${aws_rds_cluster.kore_postgres.endpoint}:5432/kore"
      },
      {
        name  = "STORAGE_BACKEND"
        value = "s3"
      },
      {
        name  = "AWS_S3_BUCKET"
        value = aws_s3_bucket.kore_storage.id
      },
      {
        name  = "AWS_REGION"
        value = var.aws_region
      },
      {
        name  = "RUST_LOG"
        value = var.log_level
      }
    ]
    logConfiguration = {
      logDriver = "awslogs"
      options = {
        "awslogs-group"         = aws_cloudwatch_log_group.kore_cloud.name
        "awslogs-region"        = var.aws_region
        "awslogs-stream-prefix" = "ecs"
      }
    }
  }])
}

# ==================== ECS Service ====================
resource "aws_ecs_service" "kore_cloud" {
  name            = "kore-cloud-${var.environment}"
  cluster         = aws_ecs_cluster.kore.id
  task_definition = aws_ecs_task_definition.kore_cloud.arn
  desired_count   = var.desired_count
  launch_type     = "FARGATE"

  network_configuration {
    subnets          = var.service_subnets
    security_groups  = [aws_security_group.alb.id]
    assign_public_ip = false
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.kore_cloud.arn
    container_name   = "kore-cloud"
    container_port   = 8000
  }

  depends_on = [aws_lb_listener.kore_cloud]
}

# ==================== ALB ====================
resource "aws_lb" "kore_cloud" {
  name               = "kore-alb-${var.environment}"
  internal           = false
  load_balancer_type = "application"
  security_groups    = [aws_security_group.alb.id]
  subnets            = var.lb_subnets

  tags = {
    Name        = "kore-alb-${var.environment}"
    Environment = var.environment
  }
}

resource "aws_lb_target_group" "kore_cloud" {
  name        = "kore-tg-${var.environment}"
  port        = 8000
  protocol    = "HTTP"
  vpc_id      = var.vpc_id
  target_type = "ip"

  health_check {
    healthy_threshold   = 2
    unhealthy_threshold = 2
    timeout             = 3
    interval            = 30
    path                = "/health"
    matcher             = "200"
  }
}

resource "aws_lb_listener" "kore_cloud" {
  load_balancer_arn = aws_lb.kore_cloud.arn
  port              = 443
  protocol          = "HTTPS"
  ssl_policy        = "ELBSecurityPolicy-TLS-1-2-2017-01"
  certificate_arn   = var.certificate_arn

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.kore_cloud.arn
  }
}

# ==================== CloudWatch ====================
resource "aws_cloudwatch_log_group" "kore_cloud" {
  name              = "/ecs/kore-cloud-${var.environment}"
  retention_in_days = 30

  tags = {
    Name        = "kore-cloud-logs"
    Environment = var.environment
  }
}

# ==================== IAM Roles ====================
resource "aws_iam_role" "ecs_task_execution_role" {
  name = "kore-ecs-task-execution-role-${var.environment}"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Action = "sts:AssumeRole"
      Effect = "Allow"
      Principal = {
        Service = "ecs-tasks.amazonaws.com"
      }
    }]
  })
}

resource "aws_iam_role_policy_attachment" "ecs_task_execution_role_policy" {
  role       = aws_iam_role.ecs_task_execution_role.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

resource "aws_iam_role" "ecs_task_role" {
  name = "kore-ecs-task-role-${var.environment}"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Action = "sts:AssumeRole"
      Effect = "Allow"
      Principal = {
        Service = "ecs-tasks.amazonaws.com"
      }
    }]
  })
}

resource "aws_iam_role_policy" "s3_access" {
  name = "kore-s3-access"
  role = aws_iam_role.ecs_task_role.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "s3:GetObject",
        "s3:PutObject",
        "s3:DeleteObject",
        "s3:ListBucket"
      ]
      Resource = [
        aws_s3_bucket.kore_storage.arn,
        "${aws_s3_bucket.kore_storage.arn}/*"
      ]
    }]
  })
}

# ==================== Security Groups ====================
resource "aws_security_group" "alb" {
  name   = "kore-alb-sg-${var.environment}"
  vpc_id = var.vpc_id

  ingress {
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  ingress {
    from_port   = 80
    to_port     = 80
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_security_group" "rds" {
  name   = "kore-rds-sg-${var.environment}"
  vpc_id = var.vpc_id

  ingress {
    from_port       = 5432
    to_port         = 5432
    protocol        = "tcp"
    security_groups = [aws_security_group.alb.id]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

# ==================== Outputs ====================
output "alb_dns_name" {
  value       = aws_lb.kore_cloud.dns_name
  description = "DNS name of the load balancer"
}

output "rds_endpoint" {
  value       = aws_rds_cluster.kore_postgres.endpoint
  description = "RDS cluster endpoint"
}

output "s3_bucket_name" {
  value       = aws_s3_bucket.kore_storage.id
  description = "S3 bucket name"
}

output "ecs_cluster_name" {
  value       = aws_ecs_cluster.kore.name
  description = "ECS cluster name"
}
