# Mixed config: This file is HCL, resources.tf.json is JSON
# Both should be loaded and merged by oxid

terraform {
  required_providers {
    random = {
      source  = "hashicorp/random"
      version = "3.7.2"
    }
    null = {
      source  = "hashicorp/null"
      version = "3.2.4"
    }
    local = {
      source  = "hashicorp/local"
      version = "2.5.3"
    }
  }
}

provider "random" {}
provider "null" {}
provider "local" {}

variable "project_name" {
  default     = "oxid-mixed-test"
  description = "Project name prefix"
  type        = string
}
