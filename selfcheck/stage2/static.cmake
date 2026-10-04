# Used only for the static self-check build (selfcheck/stage2/build.sh): Unicorn must not
# build its shared library when the C flags ask for static linking.
set(BUILD_SHARED_LIBS OFF CACHE BOOL "" FORCE)
