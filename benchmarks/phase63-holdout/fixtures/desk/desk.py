#!/usr/bin/env python3
"""String validation and ordered ticket lifecycle with rejection ordering."""
import json, os, sys

def initial():
    return {"tickets": {}, "next_id": 1, "view": "list", "selected": None,
            "rejects": 0, "opened": 0, "closed": 0, "history": []}

def path(d): return os.path.join(d, "state.json")
def load(d):
    if not os.path.exists(path(d)): return initial()
    with open(path(d), encoding="utf-8") as f: return json.load(f)
def save(d,s):
    with open(path(d),"w",encoding="utf-8") as f: json.dump(s,f,sort_keys=True,indent=2)

def envelope(s,outcome="success",error=None,events=None,extra=None):
    rows=[{k:t[k] for k in ("id","subject","status","assignee","priority")} for t in s["tickets"].values()]
    p={"outcome":outcome,"view":s["view"],"selected":s.get("selected"),
       "events":events or [],"tickets":rows,"ticket_count":len(rows),
       "open_count":sum(1 for t in s["tickets"].values() if t["status"]!="closed"),
       "closed_count":sum(1 for t in s["tickets"].values() if t["status"]=="closed"),
       "total_priority":sum(t["priority"] for t in s["tickets"].values()),
       "rejects":s["rejects"],"opened":s["opened"],"closed":s["closed"],"history":s["history"]}
    if error is not None: p["error"]=error
    if extra: p.update(extra)
    return p

def emit(d,s,outcome="success",error=None,events=None,extra=None):
    if outcome!="success": s["rejects"]+=1
    save(d,s)
    print(json.dumps(envelope(s,outcome,error,events,extra),sort_keys=True))
    return 0

def main(argv):
    if len(argv)<3 or argv[1]!="--state":
        print(json.dumps({"outcome":"error","error":"bad_usage"})); return 2
    d=argv[2]; os.makedirs(d,exist_ok=True); args=argv[3:]; s=load(d)
    if not args: print(json.dumps(envelope(s),sort_keys=True)); return 0
    cmd,rest=args[0],args[1:]

    if cmd=="open":
        if not rest: return emit(d,s,"rejected","subject_required")
        subject=rest[0]
        if len(subject)<3: return emit(d,s,"rejected","subject_too_short")
        if len(subject)>12: return emit(d,s,"rejected","subject_too_long")
        tid="t%d"%s["next_id"]; s["next_id"]+=1
        s["tickets"][tid]={"id":tid,"subject":subject,"status":"open","assignee":None,"priority":0}
        s["opened"]+=1; s["selected"]=tid; s["view"]="detail"
        s["history"].append("ticket.opened")
        return emit(d,s,events=["ticket.opened"],extra={"created_id":tid,"created_pool":"ticket"})

    if cmd in ("assign","escalate","close"):
        if not rest: return emit(d,s,"rejected","ticket_required")
        t=s["tickets"].get(rest[0])
        if t is None: return emit(d,s,"rejected","no_such_ticket")
        s["selected"]=t["id"]
        if t["status"]=="closed": return emit(d,s,"rejected","already_closed")
        if cmd=="assign":
            if len(rest)<2 or not rest[1].strip(): return emit(d,s,"rejected","agent_required")
            t["assignee"]=rest[1]; t["status"]="assigned"; s["history"].append("ticket.assigned")
            return emit(d,s,events=["ticket.assigned"])
        if cmd=="escalate":
            t["priority"]+=1; s["history"].append("ticket.escalated")
            return emit(d,s,events=["ticket.escalated"])
        t["status"]="closed"; s["closed"]+=1; s["history"].append("ticket.closed")
        return emit(d,s,events=["ticket.closed"])

    if cmd=="list":
        s["view"]="list"; s["selected"]=None; return emit(d,s)
    if cmd=="stats": return emit(d,s)
    return emit(d,s,"rejected","unknown_command")

if __name__=="__main__": sys.exit(main(sys.argv))
