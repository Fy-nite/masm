return {
  MNI_MODULE = "GUI",
  MNI_FUNCTIONS = {
    set_rax = function(args, regs)
      print("GUI.set_rax called with args:", table.concat(args, ", "))
      print("Current registers:", regs and regs.RAX or "nil")
      local v = tonumber(args[1] or "0") or 0
      return { regs = { RAX = v } }
    end,
    echo = function(args, regs)
      return table.concat(args, " ")
    end
  }
}
