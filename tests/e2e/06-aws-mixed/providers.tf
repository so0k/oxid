# AWS Mixed config: HCL providers + JSON resources
# Tests that tf.json resources merge with HCL terraform/provider blocks

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
      Project   = "oxid-e2e-mixed"
      ManagedBy = "oxid"
    }
  }
}

provider "random" {}

variable "project_name" {
  default = "oxid-e2e-mixed"
  type    = string
}
