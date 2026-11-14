import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {jsonataOnly} from '../../automation/validation.mjs';
const require=createRequire(import.meta.url),jsonata=require('jsonata');
const load=name=>JSON.parse(readFileSync(new URL(`../core/tests/fixtures/${name}.asl.json`,import.meta.url),'utf8'));
const main=load('portal-rollout-published'),rotation=load('database-rotation-published');
async function evaluate(text,input,vars={}){const expr=jsonata(text.slice(2,-2));expr.assign('states',{input,context:{Execution:{Id:'arn:execution:owner'}}});for(const [k,v] of Object.entries(vars))expr.assign(k,v);return expr.evaluate({});}
for(const name of ['portal-rollout-published','portal-rollout-infrastructure-only','database-rotation-published','database-rotation-infrastructure-only'])test(`${name} is JSONata-only and every expression compiles`,()=>{
 const machine=load(name);assert.equal(jsonataOnly(machine),true);
 function walk(v){if(typeof v==='string'&&v.startsWith('{%'))jsonata(v.slice(2,-2));else if(v&&typeof v==='object')Object.values(v).forEach(walk);}walk(machine);
});
test('portal rollout cannot request password maintenance; maintenance cannot request normal rollout',async()=>{
 const normal=main.States.ValidateRequest.Choices[0].Condition,maint=rotation.States.ValidateRequest.Choices[0].Condition;
 assert.equal(await evaluate(normal,{operation:'rotateDatabase',requestId:'valid'}),false);
 assert.equal(await evaluate(maint,{operation:'rollout',requestId:'valid'}),false);
 assert.equal(await evaluate(maint,{operation:'rotateDatabase',requestId:'valid'}),true);
 assert.equal(await evaluate(maint,{operation:'rotateDatabase',requestId:'bad spaces'}),false);
});
test('password rotation can run only after the service is fully drained; drain is bounded',async()=>{
 const condition=rotation.States.CheckDrain.Choices[0].Condition;
 const good={Failures:[],Services:[{Status:'ACTIVE',DesiredCount:0,RunningCount:0,PendingCount:0}]};
 assert.equal(await evaluate(condition,good),true);
 for(const field of ['DesiredCount','RunningCount','PendingCount']){const bad=structuredClone(good);bad.Services[0][field]=1;assert.equal(await evaluate(condition,bad),false);}
 assert.equal(await evaluate(condition,{Failures:[{Reason:'MISSING'}],Services:good.Services}),false);
 assert.equal(await evaluate(rotation.States.CheckDrain.Choices[1].Condition,{}, {drainAttempts:29}),true);
 assert.equal(rotation.States.CheckDrain.Choices[0].Next,'RotateDatabase');
 assert.equal(rotation.States.RotateDatabase.Next,'CheckBootstrapExit');
});
test('release lock has no time expiry and conditional release can remove only its execution owner',()=>{
 const acquire=main.States.AcquireReleaseLock.Arguments;
 assert.equal(main.States.RememberRequest.Next,'AcquireReleaseLock');
 assert.equal(acquire.ConditionExpression,'attribute_not_exists(LockId)');
 assert.deepEqual(Object.keys(acquire.Item).sort(),['LockId','Owner']);
 for(const state of ['ReleaseSuccessfulLock','ReleaseFailedLock']){
 const args=main.States[state].Arguments;assert.equal(args.ConditionExpression,'#owner = :owner');assert.equal(args.ExpressionAttributeValues[':owner'].S,'{% $states.context.Execution.Id %}');assert.deepEqual(args.Key.LockId,acquire.Item.LockId);
 }
 assert.equal(main.States.ReleaseSuccessfulLock.Next,'RolloutSucceeded');
 assert.equal(main.States.LockNotAcquired.Type,'Fail');
 for(const failure of ['BootstrapTaskFailed','BootstrapContainerFailed','UpdateServiceFailed','InvalidDeploymentReceipt','DescribeServiceFailed','DeploymentFailed','RolloutTimedOut','RotationFailed'])assert.equal(main.States[failure].Next,'ReleaseFailedLock');
});
