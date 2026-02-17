# AWS HCL validation - requires AWS credentials
# Creates only free/cheap resources for testing

terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "5.82.2"
    }
    random = {
      source  = "hashicorp/random"
      version = "3.7.2"
    }
  }
}

provider "aws" {
  region = "us-east-1"

  default_tags {
    tags = {
      Project   = "oxid-e2e-test"
      ManagedBy = "oxid"
    }
  }
}

provider "random" {}

variable "project_name" {
  default = "oxid-e2e"
  type    = string
}

data "aws_caller_identity" "current" {}
data "aws_region" "current" {}

resource "random_pet" "suffix" {
  length = 2
}

resource "aws_ssm_parameter" "test" {
  name  = "/oxid-e2e/${random_pet.suffix.id}/test-param"
  type  = "String"
  value = "hello-from-oxid"

  tags = {
    Name = "${var.project_name}-${random_pet.suffix.id}"
  }
}

output "account_id" {
  value = data.aws_caller_identity.current.account_id
}

output "region" {
  value = data.aws_region.current.name
}

output "parameter_name" {
  value = aws_ssm_parameter.test.name
}
