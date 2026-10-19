# F10 witness (second implementation): any access to the first byte after each record (recs[i]->data + recs[i]->cap).
# awatch, not rwatch: x86 has no read-only data breakpoints, and nothing writes these bytes after startup.
set pagination off
set confirm off
handle SIGTERM nostop noprint pass
handle SIGPIPE nostop noprint pass
awatch -l *(char *)(recs[0]->data + recs[0]->cap)
awatch -l *(char *)(recs[1]->data + recs[1]->cap)
awatch -l *(char *)(recs[2]->data + recs[2]->cap)
awatch -l *(char *)(recs[3]->data + recs[3]->cap)
commands 1-4
  printf "WITNESS_HIT\n"
  bt 3
  continue
end
printf "WATCHING\n"
continue
