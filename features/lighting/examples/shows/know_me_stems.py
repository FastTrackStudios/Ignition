import json, numpy as np, librosa
D="/run/media/Development/tools/stem-sep/out2/know-me-extended/Know Me (Extended)"
BPM=127.0; BEAT=60/BPM; BAR=4*BEAT; NB=158; sr=22050; hop=256
def load(n): return librosa.load(f"{D} ({n}).wav", sr=sr, mono=True)[0]
S={n:load(n) for n in ("Kick","Snare","Toms","Cymbals","Vocals","Bass","Guitar","Piano","Mix","Drums")}
def flux(y):
    o=librosa.onset.onset_strength(y=y, sr=sr, hop_length=hop); return o
F={n:flux(S[n]) for n in ("Kick","Snare","Toms","Cymbals")}
# grid phase: where does the kick sit against the beat
fl=F["Kick"]; n=len(fl)
def score(off): 
    idx=((np.arange(0,620)*BEAT+off)*sr/hop).astype(int); idx=idx[(idx>=0)&(idx<n)]; return fl[idx].sum()
offs=np.arange(-0.2,0.28,0.004); OFF=float(offs[np.argmax([score(o) for o in offs])])
print("grid offset s", round(OFF,3))
def t(bar,beat): return (bar-1)*BAR+(beat-1)*BEAT+OFF
win=int(0.045*sr/hop)
H={}
for k,f in F.items():
    ref=np.percentile(f,99)+1e-9
    for bar in range(1,NB+1):
        for i in range(8):
            c=int(t(bar,1+i/2)*sr/hop)
            if 0<=c-win and c+win<len(f): H[(bar,1+i/2,k)]=float(min(f[c-win:c+win+1].max()/ref,1.5))
def bar_db(y):
    out=[]
    for bar in range(1,NB+1):
        a=int(max(0,t(bar,1))*sr); b=int(t(bar+1,1)*sr); seg=y[a:b]
        out.append(float(20*np.log10(np.sqrt(np.mean(seg**2))+1e-9)) if len(seg)>100 else -90.0)
    return out
L={n:bar_db(S[n]) for n in ("Mix","Drums","Vocals","Bass","Guitar","Piano")}
json.dump({"off":OFF,"hits":[[b,be,k,v] for (b,be,k),v in H.items()],"loud":L}, open("/tmp/igtest/an/raw2.json","w"))
secs=[(1,"Count In"),(3,"Intro"),(11,"Verse 1"),(19,"PreChorus"),(27,"Chorus"),(43,"Verse 2"),(51,"PreChorus 2"),(59,"Chorus 2"),(75,"Guitar Solo"),(91,"Chorus 3"),(109,"DownChorus"),(125,"Build"),(141,"Outro"),(159,"")]
def ch(bar,i):
    k=H.get((bar,1+i/2,"Kick"),0)>0.5; s=H.get((bar,1+i/2,"Snare"),0)>0.5; tm=H.get((bar,1+i/2,"Toms"),0)>0.5; c=H.get((bar,1+i/2,"Cymbals"),0)>0.6
    return "C" if c and (k or s) else ("c" if c else ("T" if tm else ("X" if k and s else ("K" if k else ("S" if s else ".")))))
for (b0,name),(b1,_) in zip(secs,secs[1:]):
    print(f"{name} {b0}-{b1-1}")
    for bar in range(b0,b1):
        row="".join(ch(bar,i)+("" if i%2==0 else " ") for i in range(8))
        print(f" {bar:3d} {L['Mix'][bar-1]:5.0f} d{L['Drums'][bar-1]:4.0f} v{L['Vocals'][bar-1]:4.0f} b{L['Bass'][bar-1]:4.0f} g{L['Guitar'][bar-1]:4.0f} p{L['Piano'][bar-1]:4.0f} {row}")
