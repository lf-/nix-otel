#include <algorithm>
#include <config.h>
#include <dlfcn.h>
#include <eval-inline.hh>
#include <globals.hh>
#include <iostream>
#include <iterator>
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

static auto marshalActivityType(ActivityType at) -> ActivityKind {
  switch (at) {
  case actCopyPath:
    return ActivityKind::CopyPath;
  case actFileTransfer:
    return ActivityKind::FileTransfer;
  case actRealise:
    return ActivityKind::Realise;
  case actCopyPaths:
    return ActivityKind::CopyPaths;
  case actBuilds:
    return ActivityKind::Builds;
  case actBuild:
    return ActivityKind::Build;
  case actOptimiseStore:
    return ActivityKind::OptimiseStore;
  case actVerifyPaths:
    return ActivityKind::VerifyPaths;
  case actSubstitute:
    return ActivityKind::Substitute;
  case actQueryPathInfo:
    return ActivityKind::QueryPathInfo;
  case actPostBuildHook:
    return ActivityKind::PostBuildHook;
  case actBuildWaiting:
    return ActivityKind::BuildWaiting;
  default:
  case actUnknown:
    return ActivityKind::Unknown;
  }
}

static auto marshalResultType(ResultType rt) -> ResultKind {
  switch (rt) {
  case resFileLinked:
    return ResultKind::FileLinked;
  case resBuildLogLine:
    return ResultKind::BuildLogLine;
  case resUntrustedPath:
    return ResultKind::UntrustedPath;
  case resCorruptedPath:
    return ResultKind::CorruptedPath;
  case resSetPhase:
    return ResultKind::SetPhase;
  case resProgress:
    return ResultKind::Progress;
  case resSetExpected:
    return ResultKind::SetExpected;
  case resPostBuildLogLine:
    return ResultKind::PostBuildLogLine;
  default:
    return ResultKind::Unknown;
  }
}

static auto marshalField(Logger::Field const &field) -> FfiField {
  if (field.type == nix::Logger::Field::tInt) {
    return FfiField{
        .tag = FfiField::Tag::Num,
        .num = {field.i},
    };
  } else if (field.type == nix::Logger::Field::tString) {
    return FfiField{
        .tag = FfiField::Tag::String,
        .string = {FfiString{.start = field.s.data(), .len = field.s.length()}},
    };
  }
  // w/e
  __builtin_abort();
}

static auto marshalFields(Logger::Fields const &fields)
    -> std::vector<FfiField> {
  std::vector<FfiField> out{};
  std::transform(fields.begin(), fields.end(), std::back_inserter(out),
                 [](auto field) { return marshalField(field); });
  return out;
}

class OTelLogger : public Logger {
private:
  Logger *upstream;
  Context const *m_context;

public:
  OTelLogger(Logger *upstream, Context const *context)
      : upstream(upstream), m_context(context) {}
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
    start_activity(m_context, act, marshalActivityType(type), s.c_str(),
                   parent);
    upstream->startActivity(act, lvl, type, s, fields, parent);
  };

  void stopActivity(ActivityId act) override {
    end_activity(m_context, act);
    upstream->stopActivity(act);
  };

  void result(ActivityId act, ResultType type, const Fields &fields) override {
    auto fields_ = marshalFields(fields);
    on_result(m_context, act, marshalResultType(type),
              FfiFields{.start = fields_.data(), .count = fields_.size()});
    upstream->result(act, type, fields);
  };

  void writeToStdout(std::string_view s) override {
    upstream->writeToStdout(s);
  }

  std::optional<char> ask(std::string_view s) override {
    return upstream->ask(s);
  }
};

class PluginInstance {
  Context *context;
  Logger *oldLogger;

public:
  PluginInstance() {
    Logger *oldLogger = logger;
    context = initialize_plugin();
    logger = new OTelLogger(oldLogger, context);
  }

  ~PluginInstance() {
    auto toDestroy = logger;
    logger = oldLogger;
    deinitialize_plugin(context);
    delete toDestroy;
  }
};

PluginInstance x{};
