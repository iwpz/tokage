import os,subprocess,tempfile,json,signal,threading,http.server,pathlib,argparse
parser=argparse.ArgumentParser(description='Genuine OpenCode 1.x/2.x CLI contract against a local synthetic model; no upstream API calls.')
parser.add_argument('--v1',required=True);parser.add_argument('--v2',required=True);opts=parser.parse_args()
class Handler(http.server.BaseHTTPRequestHandler):
 def log_message(self,*a):pass
 def do_POST(self):
  data=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
  body={'id':'chatcmpl-fixture','object':'chat.completion','created':1,'model':'deepseek-flash','choices':[{'index':0,'message':{'role':'assistant','content':'TOKAGE_OPENCODE_CHAT_OK'},'finish_reason':'stop'}],'usage':{'prompt_tokens':12,'completion_tokens':5,'total_tokens':17}}
  self.send_response(200)
  if data.get('stream'):
   self.send_header('Content-Type','text/event-stream');self.end_headers()
   for chunk in [{'id':body['id'],'object':'chat.completion.chunk','created':1,'model':'deepseek-flash','choices':[{'index':0,'delta':{'role':'assistant','content':'TOKAGE_OPENCODE_CHAT_OK'},'finish_reason':None}]},{'id':body['id'],'object':'chat.completion.chunk','created':1,'model':'deepseek-flash','choices':[{'index':0,'delta':{},'finish_reason':'stop'}],'usage':body['usage']}]:self.wfile.write(('data: '+json.dumps(chunk)+'\n\n').encode())
   self.wfile.write(b'data: [DONE]\n\n')
  else:self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(json.dumps(body).encode())
server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
for major,exe in [(1,opts.v1),(2,opts.v2)]:
 root=pathlib.Path(tempfile.mkdtemp(prefix=f'tokage-opencode-v{major}-contract-'));home=root/'home';home.mkdir();cwd=root/'repo';cwd.mkdir()
 env=dict(os.environ,HOME=str(home),XDG_CONFIG_HOME=str(home/'.config'),XDG_DATA_HOME=str(home/'.local/share'),XDG_CACHE_HOME=str(home/'.cache'),XDG_STATE_HOME=str(home/'.local/state'),OPENCODE_CONFIG_CONTENT=json.dumps({'provider':{'deepseek':{'options':{'baseURL':f'http://127.0.0.1:{server.server_port}/v1','apiKey':'fixture-not-real'}}},'permission':{'*':'deny'},'share':'disabled','snapshot':False,'shell':'/tmp/tokage/tokage-shell-wrapper.sh'}))
 for key in list(env):
  if any(s in key for s in ['TOKEN','API_KEY','PASSWORD']) and not key.startswith('OPENCODE'):env.pop(key)
 args=[exe,'run','Reply exactly TOKAGE_OPENCODE_CHAT_OK','--format','json','--model','deepseek/deepseek-flash','--agent','plan', '--standalone' if major==2 else '--pure']
 with open(root/'stdout.jsonl','w') as out,open(root/'stderr.txt','w') as err:
  p=subprocess.Popen(args,cwd=cwd,env=env,stdout=out,stderr=err,start_new_session=True)
  try:p.wait(timeout=75)
  except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGTERM);p.wait(timeout=10)
 print(major,root,p.returncode,flush=True)
 print((root/'stdout.jsonl').read_text()[-5000:],flush=True)
 print((root/'stderr.txt').read_text()[-1000:],flush=True)
 assert p.returncode == 0
 rows=[json.loads(l) for l in (root/'stdout.jsonl').read_text().splitlines() if l.startswith('{')]
 assert any(r.get('type') == 'text' and 'TOKAGE_OPENCODE_CHAT_OK' in r.get('part',{}).get('text','') for r in rows)
 if major==1: assert any(r.get('type')=='step_finish' for r in rows)
 else:
  session=rows[0]['sessionID']
  exported=json.loads(subprocess.check_output([exe,'session','export',session,'--standalone'],env=env,cwd=cwd))
  assert exported['messages'][-1]['outcome']=='succeeded'
  assert any(m.get('type')=='assistant' and m.get('tokens',{}).get('input',0)>0 for m in exported['messages'])
 print(f'PASS OpenCode v{major} genuine CLI / local synthetic model contract',flush=True)
server.shutdown()
