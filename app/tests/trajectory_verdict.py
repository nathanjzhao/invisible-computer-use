"""Recording reuse requires actual note content in the user prompt, not boilerplate."""
import json


def memory_decisions(calls, entries):
    count=0
    for call in calls:
        messages=call.get('request',{}).get('messages',[])
        for message in messages:
            text=message.get('content','')
            if message.get('role')!='user' or not isinstance(text,str) or 'WINDOW ELEMENT TREE:' not in text:
                continue
            if '<unverified_notes>\n' not in text or '\n</unverified_notes>' not in text:
                continue
            block=text.split('<unverified_notes>\n',1)[1].split('\n</unverified_notes>',1)[0]
            try: notes=[json.loads(line) for line in block.splitlines() if line.strip()]
            except ValueError: continue
            if any(n.get('note')==e['text'] for n in notes for e in entries):count+=1;break
    return count
