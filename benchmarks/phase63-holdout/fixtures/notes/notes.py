#!/usr/bin/env python3
"""Lazy initialization: no state document exists until the first mutation."""
import json, os, sys

def initial():
    return {"notes": {}, "next_id": 1, "view": "list", "selected": None,
            "rejects": 0, "pinned": 0, "log": []}

def path(d): return os.path.join(d, "state.json")
def load(d):
    if not os.path.exists(path(d)): return initial()
    with open(path(d), encoding="utf-8") as f: return json.load(f)
def save(d,s):
    with open(path(d),"w",encoding="utf-8") as f: json.dump(s,f,sort_keys=True,indent=2)

def envelope(s,outcome="success",error=None,events=None,extra=None):
    rows=[{k:n[k] for k in ("id","text","pinned")} for n in s["notes"].values()]
    p={"outcome":outcome,"view":s["view"],"selected":s.get("selected"),
       "events":events or [],"notes":rows,"note_count":len(rows),
       "pinned_count":sum(1 for n in s["notes"].values() if n["pinned"]),
       "rejects":s["rejects"],"log":s["log"]}
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

    if cmd=="add":
        if not rest or not rest[0].strip(): return emit(d,s,"rejected","text_required")
        nid="n%d"%s["next_id"]; s["next_id"]+=1
        s["notes"][nid]={"id":nid,"text":rest[0],"pinned":False}
        s["selected"]=nid; s["view"]="detail"; s["log"].append("note.added")
        return emit(d,s,events=["note.added"],extra={"created_id":nid,"created_pool":"note"})

    if cmd in ("pin","unpin","remove"):
        if not rest: return emit(d,s,"rejected","note_required")
        n=s["notes"].get(rest[0])
        if n is None: return emit(d,s,"rejected","no_such_note")
        s["selected"]=n["id"]
        if cmd=="pin":
            if n["pinned"]: return emit(d,s,"rejected","already_pinned")
            n["pinned"]=True; s["pinned"]+=1; s["log"].append("note.pinned")
            return emit(d,s,events=["note.pinned"])
        if cmd=="unpin":
            if not n["pinned"]: return emit(d,s,"rejected","not_pinned")
            n["pinned"]=False; s["pinned"]-=1; s["log"].append("note.unpinned")
            return emit(d,s,events=["note.unpinned"])
        del s["notes"][n["id"]]
        if n["pinned"]: s["pinned"]-=1
        s["log"].append("note.removed")
        return emit(d,s,events=["note.removed"])

    if cmd=="list":
        s["view"]="list"; s["selected"]=None
        return emit(d,s)
    if cmd=="stats": return emit(d,s)
    return emit(d,s,"rejected","unknown_command")

if __name__=="__main__": sys.exit(main(sys.argv))
