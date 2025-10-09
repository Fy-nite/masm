"""
Sample Python MNI module (class-based)

Contract used by Swift Interpreter.loadModules (PythonKit):
1) Class-based (preferred):
   - Define a class with a class attribute MNI_MODULE = "name".
   - Export methods by decorating them with @mni_export() or @mni_export("alias").
   - Method signature: def method(self, args: list[str], regs: dict[str,int]) -> None | str | dict
     Return dict format: { "out": str, "regs": { regName: int, ... } }

2) Fallback (still supported):
   - MNI_MODULE = "name" and MNI_FUNCTIONS = {"fname": callable}
"""

def mni_export(name=None):
    def decorator(fn):
        export_name = name or fn.__name__
        setattr(fn, "__mni_export__", export_name)
        return fn
    return decorator


class Tool:
    MNI_MODULE = "tool"

    @mni_export()  # exports as "join"
    def join(self, args, regs):
        return ", ".join(args)

    @mni_export("set_rax")  # explicit exported name
    def set_rax(self, args, regs):
        if not args:
            return {"out": "no args"}
        s = args[0]
        try:
            val = int(s)
        except Exception:
            val = len(s)
        return {"out": f"RAX <- {val}", "regs": {"RAX": val}}
