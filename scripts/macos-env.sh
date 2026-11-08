# Source this before cargo on macOS: `. scripts/macos-env.sh`.
#
# .cargo/config.toml's [env] carries MSVC flags for whisper.cpp (-nologo -MT), and cargo's [env]
# cannot be limited to one target. Variables already in the environment win over [env], so these
# give clang the same optimization the Windows build gets.
export CMAKE_C_FLAGS_RELEASE="-O3 -DNDEBUG"
export CMAKE_CXX_FLAGS_RELEASE="-O3 -DNDEBUG"
export CMAKE_C_FLAGS_RELWITHDEBINFO="-O2 -g -DNDEBUG"
export CMAKE_CXX_FLAGS_RELWITHDEBINFO="-O2 -g -DNDEBUG"
case ":$PATH:" in
  *":/opt/homebrew/opt/rustup/bin:"*) ;;
  *) export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH" ;;
esac
