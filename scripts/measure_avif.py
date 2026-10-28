"""Benchmark-only ctypes decoder using the existing libheif.so.1; no executable.
This is not a Rust decoder and is never imported by the application.
"""
import ctypes as c
import numpy as np
class Error(c.Structure):
 _fields_=[('code',c.c_int),('subcode',c.c_int),('message',c.c_char_p)]
l=c.CDLL('libheif.so.1')
def setup(name,args,res):
 f=getattr(l,name); f.argtypes=args; f.restype=res; return f
alloc=setup('heif_context_alloc',[],c.c_void_p)
read=setup('heif_context_read_from_memory_without_copy',[c.c_void_p,c.c_void_p,c.c_size_t,c.c_void_p],Error)
primary=setup('heif_context_get_primary_image_handle',[c.c_void_p,c.POINTER(c.c_void_p)],Error)
decode=setup('heif_decode_image',[c.c_void_p,c.POINTER(c.c_void_p),c.c_int,c.c_int,c.c_void_p],Error)
plane=setup('heif_image_get_plane_readonly',[c.c_void_p,c.c_int,c.POINTER(c.c_int)],c.POINTER(c.c_uint8))
width=setup('heif_image_handle_get_width',[c.c_void_p],c.c_int); height=setup('heif_image_handle_get_height',[c.c_void_p],c.c_int)
free=setup('heif_context_free',[c.c_void_p],None); release=setup('heif_image_release',[c.c_void_p],None); handle_release=setup('heif_image_handle_release',[c.c_void_p],None)
def check(e):
 if e.code: raise ValueError(e.message)
def load(path):
 data=path.read_bytes(); buf=c.create_string_buffer(data); ctx=alloc(); handle=c.c_void_p(); image=c.c_void_p()
 try:
  check(read(ctx,buf,len(data),None)); check(primary(ctx,c.byref(handle))); check(decode(handle,c.byref(image),1,11,None)); stride=c.c_int(); ptr=plane(image,10,c.byref(stride)); w=width(handle); h=height(handle)
  return np.ctypeslib.as_array(ptr,shape=(h*stride.value,)).reshape(h,stride.value)[:,:w*4].reshape(h,w,4).copy()
 finally:
  if image: release(image)
  if handle: handle_release(handle)
  free(ctx)
