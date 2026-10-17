"""One bounded, private Windows acceptance host with explicit teardown."""
import argparse
import base64
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import secrets
import signal
import subprocess
import sys
import time

REGION = 'us-west-2'
ACCOUNT = '463200133738'
IMAGE_PARAMETER = '/aws/service/ami-windows-latest/Windows_Server-2022-English-Full-Base'
PURPOSE = {'Key':'purpose','Value':'icu-audit'}
OWNER = {'Key':'owner','Value':os.environ.get('ICU_AUDIT_OWNER','invisible-computer-use')}
PRICES = {'m7i-flex.large':0.1936,'m6i.2xlarge':0.752}

def aws(*args):
    result = subprocess.run(['aws',*args,'--profile','default','--region',REGION,'--output','json'],capture_output=True,text=True,timeout=90)
    if result.returncode: raise RuntimeError(result.stderr.strip())
    return json.loads(result.stdout) if result.stdout.strip() else {}

def create_identity(state, run):
    # Every run owns a disposable SSM role scoped to its own evidence bucket.
    trust = {'Version':'2012-10-17','Statement':[{'Effect':'Allow','Principal':{'Service':'ec2.amazonaws.com'},'Action':'sts:AssumeRole'}]}
    tags = json.dumps([PURPOSE,OWNER,{'Key':'ICUAuditRun','Value':run}])
    aws('iam','create-role','--role-name',run,'--assume-role-policy-document',json.dumps(trust),'--tags',tags)
    state['role']=run
    aws('iam','attach-role-policy','--role-name',run,'--policy-arn','arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore')
    bucket_policy = {'Version':'2012-10-17','Statement':[{'Effect':'Allow','Action':['s3:GetObject','s3:PutObject'],
        'Resource':f"arn:aws:s3:::{state['bucket']}/{run}/*"}]}
    aws('iam','put-role-policy','--role-name',run,'--policy-name','evidence','--policy-document',json.dumps(bucket_policy))
    aws('iam','create-instance-profile','--instance-profile-name',run,'--tags',tags)
    state['instance_profile']=run
    aws('iam','add-role-to-instance-profile','--instance-profile-name',run,'--role-name',run)

def delete_identity(path, state):
    profile = state.get('instance_profile')
    if profile:
        for step in (['remove-role-from-instance-profile','--instance-profile-name',profile,'--role-name',state.get('role',profile)],
                ['delete-instance-profile','--instance-profile-name',profile]):
            try: aws('iam',*step)
            except RuntimeError as error:
                if 'NoSuchEntity' not in str(error): raise
        state.pop('instance_profile',None); save(path,state)
    role = state.get('role')
    if role:
        for policy in aws('iam','list-attached-role-policies','--role-name',role)['AttachedPolicies']:
            aws('iam','detach-role-policy','--role-name',role,'--policy-arn',policy['PolicyArn'])
        for name in aws('iam','list-role-policies','--role-name',role)['PolicyNames']:
            aws('iam','delete-role-policy','--role-name',role,'--policy-name',name)
        aws('iam','delete-role','--role-name',role)
        state.pop('role',None); save(path,state)

def save(path, state):
    temp = path.with_suffix('.tmp')
    temp.write_text(json.dumps(state,indent=2)+'\n'); temp.chmod(0o600)
    temp.replace(path)

def terminate(path):
    state = json.loads(path.read_text())
    if aws('sts','get-caller-identity')['Account']!=ACCOUNT: raise RuntimeError('unexpected AWS account')
    instance = state.get('instance_id')
    if instance:
        found = aws('ec2','describe-instances','--instance-ids',instance)['Reservations'][0]['Instances'][0]
        tags = {t['Key']:t['Value'] for t in found.get('Tags',[])}
        if tags.get('ICUAuditRun') != state['run_id']: raise RuntimeError('instance ownership tag mismatch')
        if found['State']['Name'] != 'terminated':
            aws('ec2','terminate-instances','--instance-ids',instance)
        deadline = time.monotonic()+300
        while time.monotonic()<deadline:
            current=aws('ec2','describe-instances','--instance-ids',instance)['Reservations'][0]['Instances'][0]
            if current['State']['Name']=='terminated': break
            time.sleep(10)
        else: raise RuntimeError('termination is still pending')
    delete_identity(path,state)
    group = state.get('security_group_id')
    if group:
        aws('ec2','delete-security-group','--group-id',group)
        state.pop('security_group_id',None)
        save(path,state)
    bucket=state.get('bucket')
    if bucket:
        tags={tag['Key']:tag['Value'] for tag in aws('s3api','get-bucket-tagging','--bucket',bucket)['TagSet']}
        if tags.get('ICUAuditRun')!=state['run_id']: raise RuntimeError('bucket ownership tag mismatch')
        prefix=state['run_id']+'/'
        while True:
            listing=aws('s3api','list-objects-v2','--bucket',bucket,'--prefix',prefix)
            keys=[{'Key':item['Key']} for item in listing.get('Contents',[])]
            if not keys: break
            aws('s3api','delete-objects','--bucket',bucket,'--delete',json.dumps({'Objects':keys,'Quiet':True}))
        aws('s3api','delete-bucket','--bucket',bucket)
        state.pop('bucket',None); save(path,state)
    state['status']='terminated'; save(path,state)

def launch(path, subnet, hours, instance_type):
    if path.exists(): raise RuntimeError('a run journal already exists')
    if aws('sts','get-caller-identity')['Account']!=ACCOUNT: raise RuntimeError('unexpected AWS account')
    # Lowercase: the run id also names the S3 bucket.
    run = 'icu-win-'+dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dt%H%M%Sz')+'-'+secrets.token_hex(3)
    expires = dt.datetime.now(dt.timezone.utc)+dt.timedelta(hours=hours)
    script = Path(__file__).with_name('bootstrap.ps1').read_bytes()
    image = aws('ssm','get-parameter','--name',IMAGE_PARAMETER)['Parameter']['Value']
    state = {'run_id':run,'account':ACCOUNT,'region':REGION,'image_parameter':IMAGE_PARAMETER,'image_id':image,'instance_type':instance_type,
        'expires_at':expires.isoformat(),'status':'preparing','bootstrap_sha256':hashlib.sha256(script).hexdigest(),
        'on_demand_windows_usd_per_hour':PRICES[instance_type],'compute_budget_hours':hours,
        'scope':'isolated Windows build and native app acceptance; no public inbound access'}
    path.parent.mkdir(parents=True,exist_ok=True,mode=0o700)
    if path.parent.stat().st_mode & 0o077: raise RuntimeError('use a private run directory with mode 0700')
    save(path,state)
    info = aws('ec2','describe-subnets','--subnet-ids',subnet)['Subnets'][0]
    password = 'Aa1!'+secrets.token_urlsafe(24)
    encoded = base64.b64encode(script).decode()
    userdata = rf'''<powershell>
$ErrorActionPreference='Stop'
New-Item -ItemType Directory -Force C:\ICUAudit | Out-Null
[IO.File]::WriteAllBytes('C:\ICUAudit\bootstrap.ps1',[Convert]::FromBase64String('{encoded}'))
& C:\ICUAudit\bootstrap.ps1 -RunId '{run}' -AuditPassword '{password}' -ExpiresAt '{expires.isoformat()}'
</powershell>
'''
    if len(userdata.encode()) > 16*1024: raise RuntimeError('EC2 user data exceeds its limit')
    secret_file = path.parent/'userdata.ps1'
    with os.fdopen(os.open(secret_file,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600),'w') as output:
        output.write(userdata)
    try:
        state['bucket']=run; save(path,state)
        aws('s3api','create-bucket','--bucket',run,'--create-bucket-configuration',json.dumps({'LocationConstraint':REGION}))
        aws('s3api','put-public-access-block','--bucket',run,'--public-access-block-configuration',
            'BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true')
        aws('s3api','put-bucket-tagging','--bucket',run,'--tagging',json.dumps({'TagSet':[PURPOSE,OWNER,{'Key':'ICUAuditRun','Value':run}]}))
        create_identity(state,run); save(path,state)
        group = aws('ec2','create-security-group','--vpc-id',info['VpcId'],'--group-name',run,
            '--description','Temporary ICU native acceptance. No inbound rules.')['GroupId']
        state['security_group_id']=group; save(path,state)
        tags=[{'Key':'Name','Value':run},PURPOSE,OWNER,{'Key':'ICUAuditRun','Value':run},{'Key':'ExpiresAt','Value':expires.isoformat()},{'Key':'expires','Value':expires.isoformat()}]
        aws('ec2','create-tags','--resources',group,'--tags',json.dumps(tags))
        # A new instance profile takes several seconds to become usable by EC2.
        time.sleep(15)
        response=aws('ec2','run-instances','--image-id',image,'--instance-type',instance_type,
            '--subnet-id',subnet,'--security-group-ids',group,'--associate-public-ip-address',
            '--iam-instance-profile',json.dumps({'Name':state['instance_profile']}),'--client-token',run,
            '--instance-initiated-shutdown-behavior','terminate',
            '--metadata-options',json.dumps({'HttpTokens':'required','HttpEndpoint':'enabled'}),
            '--block-device-mappings',json.dumps([{'DeviceName':'/dev/sda1','Ebs':{'VolumeSize':100,'VolumeType':'gp3','Encrypted':True,'DeleteOnTermination':True}}]),
            '--tag-specifications',json.dumps([{'ResourceType':'instance','Tags':tags},{'ResourceType':'volume','Tags':tags}]),
            '--user-data','file://'+str(secret_file))
        state['instance_id']=response['Instances'][0]['InstanceId']; state['status']='bootstrapping'; save(path,state)
        log = (path.parent/'watchdog.log').open('ab')
        watcher=subprocess.Popen([sys.executable,str(Path(__file__).resolve()),'watchdog',str(path.resolve())],
            stdin=subprocess.DEVNULL,stdout=log,stderr=log,start_new_session=True)
        log.close(); state['watchdog_pid']=watcher.pid; save(path,state)
        print(json.dumps({k:state[k] for k in ['run_id','instance_id','expires_at','watchdog_pid']}))
    except Exception:
        if not state.get('instance_id'):
            found=aws('ec2','describe-instances','--filters',f'Name=client-token,Values={run}').get('Reservations',[])
            instances=[i for r in found for i in r['Instances']]
            if len(instances)==1: state['instance_id']=instances[0]['InstanceId']; save(path,state)
        terminate(path)
        raise
    finally:
        secret_file.unlink(missing_ok=True)

def restart_watchdog(path):
    state=json.loads(path.read_text()); old=state.get('watchdog_pid')
    if old:
        command=subprocess.check_output(['ps','-p',str(old),'-o','command='],text=True)
        if 'cloud_audit.py watchdog '+str(path.resolve()) not in command:
            raise RuntimeError('watchdog process identity mismatch')
    log=(path.parent/'watchdog.log').open('ab')
    child=subprocess.Popen([sys.executable,str(Path(__file__).resolve()),'watchdog',str(path.resolve())],
        stdin=subprocess.DEVNULL,stdout=log,stderr=log,start_new_session=True)
    log.close();state['watchdog_pid']=child.pid;save(path,state)
    if old:os.kill(old,signal.SIGTERM)
    print(json.dumps({'watchdog_pid':child.pid}))

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('action',choices=['launch','status','terminate','watchdog','restart-watchdog'])
    parser.add_argument('journal',type=Path)
    parser.add_argument('--subnet')
    parser.add_argument('--hours',type=float,default=3)
    # The audit account's free plan admits only free-tier instance types.
    parser.add_argument('--instance-type',choices=sorted(PRICES),default='m7i-flex.large')
    args=parser.parse_args()
    if args.action=='launch': launch(args.journal,args.subnet,args.hours,args.instance_type)
    elif args.action=='terminate': terminate(args.journal)
    elif args.action=='restart-watchdog': restart_watchdog(args.journal)
    elif args.action=='status':
        state=json.loads(args.journal.read_text())
        result=aws('ec2','describe-instances','--instance-ids',state['instance_id'])
        instance=result['Reservations'][0]['Instances'][0]
        print(json.dumps({'instance_id':state['instance_id'],'state':instance['State']['Name'],'expires_at':state['expires_at']}))
    else:
        while True:
            state=json.loads(args.journal.read_text())
            if state['status']=='terminated': return
            if dt.datetime.now(dt.timezone.utc)>=dt.datetime.fromisoformat(state['expires_at']): break
            time.sleep(30)
        terminate(args.journal)

if __name__=='__main__': main()
