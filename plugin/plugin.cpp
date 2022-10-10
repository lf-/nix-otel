#include <config.h>
#include <dlfcn.h>
#include <eval-inline.hh>
#include <globals.hh>
#include <iostream>
#include <optional>
#include <primops.hh>
#include <string_view>

#if HAVE_BOEHMGC

#include <gc/gc.h>
#include <gc/gc_cpp.h>

#endif

#include "./nix_otel_plugin.h"

using namespace nix;

extern "C" void discourage_linker_from_discarding() {}

class OTelLogger : public Logger {
private:
  Logger *upstream;
  Context const *context;

public:
  OTelLogger(Logger *upstream, Context const *context) : upstream(upstream), context(context) {}
  ~OTelLogger() = default;

  void stop() override { upstream->stop(); }

  bool isVerbose() override { return upstream->isVerbose(); }

  void log(Verbosity lvl, const FormatOrString &fs) override {
    upstream->log(lvl, fs);
  }

  void logEI(const ErrorInfo &ei) override { upstream->logEI(ei); }

  void warn(const std::string &msg) override { upstream->log(msg); }

  void startActivity(ActivityId act, Verbosity lvl, ActivityType type,
                     const std::string &s, const Fields &fields,
                     ActivityId parent) override {
    // FIXME: remove static_cast and replace with cleaner marshalling
    start_activity(context, act, static_cast<ActivityKind>(type), s.c_str(),
                   parent);
    upstream->startActivity(act, lvl, type, s, fields, parent);
  };

  void stopActivity(ActivityId act) override {
    end_activity(context, act);
    upstream->stopActivity(act);
  };

  void result(ActivityId act, ResultType type, const Fields &fields) override {
    upstream->result(act, type, fields);
  };

  void writeToStdout(std::string_view s) override {
    upstream->writeToStdout(s);
  }

  std::optional<char> ask(std::string_view s) override {
    return upstream->ask(s);
  }
};

int install() {
  Logger *oldLogger = logger;
  auto context = initialize_plugin();
  logger = new OTelLogger(oldLogger, context);
  return 0;
}

int x = install();
