"""本地 Python 任务的父进程退出与 Windows 子进程边界。"""
import ctypes
import os
import threading


def guard_parent(pid):
    if os.name != 'nt' or not pid:
        return
    # Windows Job 只包含当前 worker 及其后代，关闭句柄会清理提取工具。
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.CreateJobObjectW.restype = ctypes.c_void_p
    kernel.OpenProcess.restype = ctypes.c_void_p
    kernel.GetCurrentProcess.restype = ctypes.c_void_p
    kernel.SetInformationJobObject.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p, ctypes.c_uint]
    kernel.AssignProcessToJobObject.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint]
    job = kernel.CreateJobObjectW(None, None)
    info = ctypes.create_string_buffer(144)
    ctypes.c_uint.from_buffer(info, 16).value = 0x2000
    if not kernel.SetInformationJobObject(job, 9, info, len(info)) or not kernel.AssignProcessToJobObject(job, kernel.GetCurrentProcess()):
        raise OSError('Cannot establish worker process boundary')
    parent = kernel.OpenProcess(0x00100000, False, pid)
    if not parent:
        raise OSError('Parent process is unavailable')
    def wait():
        kernel.WaitForSingleObject(parent, 0xffffffff)
        os._exit(2)
    threading.Thread(target=wait, daemon=True).start()
