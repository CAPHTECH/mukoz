set pagination off
set confirm off
handle SIGTERM nostop noprint pass
handle SIGPIPE nostop noprint pass
awatch -l items[0].p->needs_eol
commands
  if items[0].released
    printf "WITNESS_HIT\n"
    bt 3
  end
  continue
end
awatch -l items[1].p->needs_eol
commands
  if items[1].released
    printf "WITNESS_HIT\n"
    bt 3
  end
  continue
end
awatch -l items[2].p->needs_eol
commands
  if items[2].released
    printf "WITNESS_HIT\n"
    bt 3
  end
  continue
end
awatch -l items[3].p->needs_eol
commands
  if items[3].released
    printf "WITNESS_HIT\n"
    bt 3
  end
  continue
end
printf "WATCHING\n"
continue
