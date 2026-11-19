# Kit pieces on the eighth-note grid of the set's own tempo map, and loudness per bar.
import json, sys, numpy as np, librosa
D="/run/media/Development/tools/stem-sep/out2/know-me-extended/Know Me (Extended)"
G=json.load(open("/tmp/igtest/an/grid.json")); BS=G["bar_starts"]; NB=len(BS)-1
sr=22050; hop=256
def load(n): return librosa.load(f"{D} ({n}).wav", sr=sr, mono=True)[0]
S={n:load(n) for n in ("Kick","Snare","Toms","Cymbals","Vocals","Mix","Drums")}
F={n:librosa.onset.onset_strength(y=S[n], sr=sr, hop_length=hop) for n in ("Kick","Snare","Toms","Cymbals")}
def t(bar,beat,off=0.0):
    a,b=BS[bar-1],BS[bar]; return a+(b-a)*(beat-1)/4+off
fl=F["Kick"]+F["Snare"]
def score(off):
    tot=0
    for bar in range(27,105):
        for beat in (1,2,3,4):
            c=int(t(bar,beat,off)*sr/hop)
            if 0<=c<len(fl): tot+=fl[c]
    return tot
offs=np.arange(-0.15,0.35,0.004); OFF=float(offs[np.argmax([score(o) for o in offs])])
win=int(0.04*sr/hop); H=[]
for k,f in F.items():
    ref=np.percentile(f,99)+1e-9
    for bar in range(1,NB+1):
        for i in range(8):
            c=int(t(bar,1+i/2,OFF)*sr/hop)
            if 0<=c-win and c+win<len(f): H.append([bar,1+i/2,k,float(min(f[c-win:c+win+1].max()/ref,1.5))])
def bar_db(y):
    out=[]
    for bar in range(1,NB+1):
        seg=y[int(max(0,t(bar,1,OFF))*sr):int(t(bar,5,OFF)*sr)]
        out.append(float(20*np.log10(np.sqrt(np.mean(seg**2))+1e-9)) if len(seg)>100 else -90.0)
    return out
json.dump({"off":OFF,"hits":H,"loud":{n:bar_db(S[n]) for n in ("Mix","Drums","Vocals")},"sections":G["sections"]}, open("/tmp/igtest/an/chart.json","w"))
print("grid offset", round(OFF,3), "bars", NB)
hh={(b,be,k):v for b,be,k,v in H}
for bar in list(range(105,112))+list(range(123,160)):
    row="".join(("C" if hh.get((bar,1+i/2,"Cymbals"),0)>0.6 else "T" if hh.get((bar,1+i/2,"Toms"),0)>0.5 else "S" if hh.get((bar,1+i/2,"Snare"),0)>0.5 else "K" if hh.get((bar,1+i/2,"Kick"),0)>0.5 else ".")+("" if i%2==0 else " ") for i in range(8))
    L=json.load(open("/tmp/igtest/an/chart.json"))["loud"] if bar==105 else L
    print(bar, round(L["Mix"][bar-1]), round(L["Drums"][bar-1]), round(L["Vocals"][bar-1]), row)
