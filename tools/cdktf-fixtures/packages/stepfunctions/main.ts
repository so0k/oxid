// Adapted from TerraConstructs integ/aws/stepfunctions: lambda-invoke.ts
import { App, LocalBackend } from "cdktf";
import { aws, Duration } from "terraconstructs";

const app = new App();
const stack = new aws.AwsStack(app, "stepfunctions", {
  gridUUID: "12345678-1234",
  environmentName: "test",
  providerConfig: { region: "us-east-1" },
});
new LocalBackend(stack, { path: "stepfunctions.tfstate" });

// Lambda: submit job
const submitFn = new aws.compute.LambdaFunction(stack, "SubmitJobLambda", {
  handler: "index.handler",
  code: aws.compute.Code.fromInline(
    `exports.handler = async (event) => ({ jobId: "job-123", status: "SUBMITTED" });`,
  ),
  runtime: aws.compute.Runtime.NODEJS_18_X,
});

// Lambda: check status
const checkFn = new aws.compute.LambdaFunction(stack, "CheckStatusLambda", {
  handler: "index.handler",
  code: aws.compute.Code.fromInline(
    `exports.handler = async (event) => ({ status: "SUCCEEDED" });`,
  ),
  runtime: aws.compute.Runtime.NODEJS_18_X,
});

// Step Functions tasks
const submitJob = new aws.compute.tasks.LambdaInvoke(stack, "SubmitJob", {
  lambdaFunction: submitFn,
  payload: aws.compute.TaskInput.fromObject({
    execId: aws.compute.JsonPath.executionId,
    execInput: aws.compute.JsonPath.executionInput,
    execName: aws.compute.JsonPath.executionName,
  }),
  outputPath: "$.Payload",
});

const checkJobState = new aws.compute.tasks.LambdaInvoke(stack, "CheckJobState", {
  lambdaFunction: checkFn,
  resultSelector: {
    status: aws.compute.JsonPath.stringAt("$.Payload.status"),
  },
});

const isComplete = new aws.compute.Choice(stack, "JobComplete?");
const jobFailed = new aws.compute.Fail(stack, "JobFailed", {
  cause: "Job Failed",
  error: "Received a status that was not 200",
});
const finalStatus = new aws.compute.Pass(stack, "FinalStep");

const chain = aws.compute.Chain.start(submitJob)
  .next(checkJobState)
  .next(
    isComplete
      .when(aws.compute.Condition.stringEquals("$.status", "FAILED"), jobFailed)
      .when(
        aws.compute.Condition.stringEquals("$.status", "SUCCEEDED"),
        finalStatus,
      ),
  );

new aws.compute.StateMachine(stack, "Pipeline", {
  definitionBody: aws.compute.DefinitionBody.fromChainable(chain),
  timeout: Duration.seconds(30),
  registerOutputs: true,
  outputName: "state_machine",
});

app.synth();
