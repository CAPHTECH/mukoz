# F10 witness: any access to the first byte after each item allocation (items[i].p + items[i].cap).
# awatch, not rwatch: x86 has no read-only data breakpoints, and nothing writes these bytes after startup.
set pagination off
set confirm off
handle SIGTERM nostop noprint pass
handle SIGPIPE nostop noprint pass
awatch -l *(char *)(items[0].p + items[0].cap)
awatch -l *(char *)(items[1].p + items[1].cap)
awatch -l *(char *)(items[2].p + items[2].cap)
awatch -l *(char *)(items[3].p + items[3].cap)
commands 1-4
  printf "WITNESS_HIT\n"
  bt 3
  continue
end
printf "WATCHING\n"
continue
