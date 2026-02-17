// Adapted from TerraConstructs integ/aws/storage: table.autoscaling.ts, table.policy.ts
import { App, LocalBackend, TerraformOutput } from "cdktf";
import { Construct } from "constructs";
import { aws } from "terraconstructs";

class StorageAutoscalingStack extends aws.AwsStack {
  constructor(scope: Construct, id: string, props: aws.AwsStackProps) {
    super(scope, id, props);

    // --- Pattern 1: Table with autoscaling (from table.autoscaling.ts) ---
    const table = new aws.storage.Table(this, "StateTable", {
      partitionKey: { name: "pk", type: aws.storage.AttributeType.STRING },
    });

    const readScaling = table.autoScaleReadCapacity({
      minCapacity: 1,
      maxCapacity: 10,
    });

    readScaling.scaleOnUtilization({
      targetUtilizationPercent: 30,
    });

    readScaling.scaleOnSchedule("ScaleUpMorning", {
      schedule: aws.compute.Schedule.cron({ hour: "8", minute: "0" }),
      minCapacity: 5,
    });

    readScaling.scaleOnSchedule("ScaleDownNight", {
      schedule: aws.compute.Schedule.cron({ hour: "20", minute: "0" }),
      maxCapacity: 3,
    });

    new TerraformOutput(this, "StateTableName", {
      value: table.tableName,
      staticId: true,
    });

    new TerraformOutput(this, "StateTableArn", {
      value: table.tableArn,
      staticId: true,
    });

    // --- Pattern 2: Table with resource policy and grants (from table.policy.ts) ---
    const doc = new aws.iam.PolicyDocument(this, "ResourcePolicy", {
      statement: [
        new aws.iam.PolicyStatement({
          actions: ["dynamodb:*"],
          principals: [new aws.iam.AccountRootPrincipal()],
          resources: ["*"],
        }),
      ],
    });

    const policyTable = new aws.storage.Table(this, "PolicyTable", {
      partitionKey: { name: "id", type: aws.storage.AttributeType.STRING },
      resourcePolicy: doc,
      registerOutputs: true,
      outputName: "policy_table",
    });

    const grantTable = new aws.storage.Table(this, "GrantTable", {
      partitionKey: { name: "PK", type: aws.storage.AttributeType.STRING },
      registerOutputs: true,
      outputName: "grant_table",
    });

    grantTable.grantReadData(new aws.iam.AccountPrincipal(this.account));

    const testRole = new aws.iam.Role(this, "TestRole", {
      assumedBy: new aws.iam.AccountPrincipal(this.account),
      registerOutputs: true,
      outputName: "test_role",
    });

    policyTable.grantReadWriteData(testRole);
  }
}

const app = new App();
new StorageAutoscalingStack(app, "storage-autoscaling", {
  gridUUID: "12345678-1234",
  environmentName: "test",
  providerConfig: { region: "us-east-1" },
});

app.synth();
