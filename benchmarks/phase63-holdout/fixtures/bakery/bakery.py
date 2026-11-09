#!/usr/bin/env python3
"""Numeric inventory: reservations, derived availability, rejection ordering."""
import json, os, sys

def initial():
    return {"batches": {}, "next_id": 101, "view": "list", "selected": None,
            "rejects": 0, "baked": 0, "shipped": 0, "history": []}

def path(d): return os.path.join(d, "state.json")
def load(d):
    if not os.path.exists(path(d)): return initial()
    with open(path(d), encoding="utf-8") as f: return json.load(f)
def save(d,s):
    with open(path(d),"w",encoding="utf-8") as f: json.dump(s,f,sort_keys=True,indent=2)

def envelope(s,outcome="success",error=None,events=None,extra=None):
    rows=[]
    for b in s["batches"].values():
        rows.append({k:b[k] for k in ("id","item","quantity","reserved","stage")})
    p={"outcome":outcome,"view":s["view"],"selected":s.get("selected"),
       "events":events or [],"batches":rows,"batch_count":len(rows),
       "total_quantity":sum(b["quantity"] for b in s["batches"].values()),
       "total_reserved":sum(b["reserved"] for b in s["batches"].values()),
       "shipped_count":sum(1 for b in s["batches"].values() if b["stage"]=="shipped"),
       "rejects":s["rejects"],"baked":s["baked"],"shipped":s["shipped"],
       "history":s["history"]}
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

    if cmd=="bake":
        if not rest or len(rest[0].strip())<3:
            return emit(d,s,"rejected","item_too_short")
        bid="b%d"%s["next_id"]; s["next_id"]+=1
        s["batches"][bid]={"id":bid,"item":rest[0],"quantity":3,"reserved":0,"stage":"ready"}
        s["baked"]+=3; s["selected"]=bid; s["view"]="detail"
        s["history"].append("batch.baked")
        return emit(d,s,events=["batch.baked"],extra={"created_id":bid,"created_pool":"batch"})

    if cmd in ("reserve","release","ship"):
        if not rest: return emit(d,s,"rejected","batch_required")
        b=s["batches"].get(rest[0])
        if b is None: return emit(d,s,"rejected","no_such_batch")
        s["selected"]=b["id"]
        if cmd=="reserve":
            if b["quantity"]-b["reserved"]<2:
                return emit(d,s,"rejected","insufficient_stock")
            b["reserved"]+=2; s["history"].append("batch.reserved")
            return emit(d,s,events=["batch.reserved"])
        if b["stage"]=="shipped":
            return emit(d,s,"rejected","already_shipped")
        if b["reserved"]==0:
            return emit(d,s,"rejected","nothing_reserved")
        if cmd=="release":
            b["reserved"]=0; s["history"].append("batch.released")
            return emit(d,s,events=["batch.released"])
        b["stage"]="shipped"; s["shipped"]+=1; s["history"].append("batch.shipped")
        return emit(d,s,events=["batch.shipped"])

    if cmd=="list":
        s["view"]="list"; s["selected"]=None; return emit(d,s)
    if cmd=="stats": return emit(d,s)
    return emit(d,s,"rejected","unknown_command")

if __name__=="__main__": sys.exit(main(sys.argv))
