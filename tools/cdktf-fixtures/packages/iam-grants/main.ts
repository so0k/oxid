// Adapted from TerraConstructs integ/aws/iam: role.ts, managed-policy.ts, condition-with-ref.ts
import { App, Lazy, LocalBackend, TerraformVariable } from "cdktf";
import { aws } from "terraconstructs";

const app = new App();
const stack = new aws.AwsStack(app, "iam-grants", {
  gridUUID: "12345678-1234",
  environmentName: "test",
  providerConfig: { region: "us-east-1" },
});
new LocalBackend(stack, { path: "iam-grants.tfstate" });

// --- Pattern 1: Role with policy attachments (from role.ts) ---
const appRole = new aws.iam.Role(stack, "AppRole", {
  assumedBy: new aws.iam.ServicePrincipal("sqs.amazonaws.com"),
  registerOutputs: true,
  outputName: "app_role",
});

appRole.addToPolicy(
  new aws.iam.PolicyStatement({
    resources: ["*"],
    actions: ["sqs:SendMessage"],
  }),
);

const inlinePolicy = new aws.iam.Policy(stack, "AppInlinePolicy", {
  policyName: "InlineAccess",
});
inlinePolicy.addStatements(
  new aws.iam.PolicyStatement({ actions: ["ec2:Describe*"], resources: ["*"] }),
);
inlinePolicy.attachToRole(appRole);

// --- Pattern 2: Managed policy with grant chains (from managed-policy.ts) ---
const managedPolicy = new aws.iam.ManagedPolicy(stack, "SharedPolicy", {
  managedPolicyName: "OxidSharedAccess",
  description: "Shared access policy for Oxid services",
  path: "/oxid/",
  registerOutputs: true,
  outputName: "shared_policy",
});
managedPolicy.addStatements(
  new aws.iam.PolicyStatement({
    resources: ["*"],
    actions: ["s3:GetObject", "s3:ListBucket"],
  }),
);

const ciRole = new aws.iam.Role(stack, "CiRole", {
  assumedBy: new aws.iam.AccountRootPrincipal(),
  registerOutputs: true,
  outputName: "ci_role",
});
ciRole.grantAssumeRole(managedPolicy.grantPrincipal);
managedPolicy.attachToRole(ciRole);

// Attach an AWS managed policy
const securityAudit = aws.iam.ManagedPolicy.fromAwsManagedPolicyName(
  stack,
  "SecurityAudit",
  "SecurityAudit",
);
securityAudit.attachToRole(ciRole);

// Grant.addToPrincipal pattern
const lambdaPolicy = new aws.iam.ManagedPolicy(stack, "LambdaPolicy", {
  registerOutputs: true,
  outputName: "lambda_policy",
});
lambdaPolicy.addStatements(
  new aws.iam.PolicyStatement({
    resources: ["*"],
    actions: ["lambda:InvokeFunction"],
  }),
);

aws.iam.Grant.addToPrincipal({
  actions: ["iam:PassRole"],
  resourceArns: [appRole.roleArn],
  grantee: lambdaPolicy,
});

// --- Pattern 3: Conditional role with variable ref (from condition-with-ref.ts) ---
const tagName = new TerraformVariable(stack, "PrincipalTag", {
  default: "developer",
});

const conditionalPrincipal = new aws.iam.AccountRootPrincipal().withConditions({
  test: "StringEquals",
  variable: Lazy.stringValue({
    produce: () => `aws:PrincipalTag/${tagName.value}`,
  }),
  values: ["true"],
});

new aws.iam.Role(stack, "ConditionalRole", {
  assumedBy: conditionalPrincipal,
  registerOutputs: true,
  outputName: "conditional_role",
});

// --- Pattern 4: Role with external ID (from role.ts) ---
new aws.iam.Role(stack, "ExternalIdRole", {
  assumedBy: new aws.iam.AccountRootPrincipal(),
  externalIds: ["supply-me"],
  registerOutputs: true,
  outputName: "external_id_role",
});

// --- Pattern 5: Role with org principal (from role.ts) ---
new aws.iam.Role(stack, "OrgRole", {
  assumedBy: new aws.iam.OrganizationPrincipal("o-1234"),
  registerOutputs: true,
  outputName: "org_role",
});

app.synth();
