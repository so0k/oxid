# Pure HCL validation - no cloud credentials needed
# Uses random, null, and local providers only

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
  default     = "oxid-hcl-test"
  description = "Project name prefix"
  type        = string
}

resource "random_pet" "name" {
  length    = 2
  prefix    = var.project_name
  separator = "-"
}

resource "random_integer" "port" {
  min = 8000
  max = 9000
}

resource "random_password" "secret" {
  length  = 16
  special = true
}

resource "null_resource" "example" {
  triggers = {
    pet_name = random_pet.name.id
    port     = random_integer.port.result
  }
}

resource "local_file" "config" {
  filename = "${path.module}/generated/config.txt"
  content  = "name=${random_pet.name.id}\nport=${random_integer.port.result}\n"

  depends_on = [null_resource.example]
}

output "pet_name" {
  value = random_pet.name.id
}

output "port" {
  value = random_integer.port.result
}

output "password" {
  value     = random_password.secret.result
  sensitive = true
}
